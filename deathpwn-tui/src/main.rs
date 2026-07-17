//! deathpwn-tui: the ratatui front end. Owns the tokio runtime, the terminal,
//! and all rendering. No business logic — it plumbs crossterm key events into
//! the core `Engine` and draws the `EngineEvent`s streamed back.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use deathpwn_core::clock::{Clock, SystemClock};
use deathpwn_core::config::Config;
use deathpwn_core::engine::{Engine, EngineEvent};
use deathpwn_core::error::Result;
use deathpwn_core::exec::ShellRunner;
use deathpwn_core::providers::{AiProvider, FailoverClient, OpenAiClient};

mod app;
mod ui;

use app::{App, Job, StatusBar};

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "-h" || arg == "--help") {
        println!("deathPWN - Agentic AI Coding Assistant / Offensive-Security Terminal");
        println!();
        println!("Usage: deathPWN [OPTIONS]");
        println!();
        println!("Options:");
        println!("  --no-cache, --disable-cache  Disable in-memory command caching");
        println!("  --cache, --enable-cache      Enable in-memory command caching");
        println!("  --clear-history              Clear all previous session command logs/history");
        println!("  -h, --help                   Print help information");
        return Ok(());
    }

    if args.iter().any(|arg| arg == "--no-cache" || arg == "--disable-cache") {
        std::env::set_var("DEATHPWN_DISABLE_CACHE", "true");
    } else if args.iter().any(|arg| arg == "--cache" || arg == "--enable-cache") {
        std::env::set_var("DEATHPWN_DISABLE_CACHE", "false");
    }

    let mut history_val = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--history" {
            if i + 1 < args.len() {
                history_val = Some(args[i + 1].clone());
                i += 2;
            } else {
                eprintln!("Error: --history requires an argument (on/off/clear)");
                std::process::exit(1);
            }
        } else if args[i].starts_with("--history=") {
            let val = args[i].split_at("--history=".len()).1.to_string();
            history_val = Some(val);
            i += 1;
        } else {
            i += 1;
        }
    }

    let mut clear_history_flag = args.iter().any(|arg| arg == "--clear-history");

    if let Some(val) = history_val {
        match val.as_str() {
            "on" => {
                std::env::set_var("DEATHPWN_DISABLE_HISTORY", "false");
            }
            "off" => {
                std::env::set_var("DEATHPWN_DISABLE_HISTORY", "true");
            }
            "clear" => {
                clear_history_flag = true;
            }
            _ => {
                eprintln!("Error: invalid value for --history: '{}'. Expected 'on', 'off', or 'clear'.", val);
                std::process::exit(1);
            }
        }
    }

    load_dotenv();
    let config = Config::from_env()?;

    if clear_history_flag {
        let artifacts_dir = config.artifacts_dir.clone();
        if artifacts_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&artifacts_dir) {
                eprintln!("Error clearing history directory '{}': {}", artifacts_dir.display(), e);
                std::process::exit(1);
            } else {
                println!("Cleared history directory: {}", artifacts_dir.display());
            }
        } else {
            println!("History directory does not exist or is already empty.");
        }
        return Ok(());
    }
    let provider_label = config.provider_a.model.clone();

    let provider_a: Arc<dyn AiProvider> = Arc::new(OpenAiClient::new(
        config.provider_a.url.clone(),
        config.provider_a.key.clone(),
        config.provider_a.model.clone(),
        "provider-a",
        config.http_timeout_secs,
    )?);
    let provider_b: Arc<dyn AiProvider> = Arc::new(OpenAiClient::new(
        config.provider_b.url.clone(),
        config.provider_b.key.clone(),
        config.provider_b.model.clone(),
        "provider-b",
        config.http_timeout_secs,
    )?);
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let engine_ai =
        FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone());

    let mut engine = Engine::new(
        ShellRunner::new(config.shell.clone()),
        engine_ai,
    );

    let (job_tx, mut job_rx) = mpsc::channel::<Job>(64);
    let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(1024);
    let (key_tx, mut key_rx) = mpsc::channel::<KeyEvent>(64);

    tokio::spawn(async move {
        while let Some(job) = job_rx.recv().await {
            let _ = engine
                .handle_line(&job.line, event_tx.clone(), job.cancel)
                .await;
        }
    });

    thread::spawn(move || loop {
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                if let Ok(Event::Key(key)) = event::read() {
                    if key_tx.blocking_send(key).is_err() {
                        break;
                    }
                }
            }
            Ok(false) => {}
            Err(_) => break,
        }
    });

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new(job_tx, StatusBar::new(provider_label));

    let spinner_interval = tokio::time::Duration::from_millis(80);

    let result: Result<()> = loop {
        app.status.tick();
        if let Err(e) = terminal.draw(|f| ui::draw(f, &app)) {
            break Err(e.into());
        }
        if app.should_quit {
            break Ok(());
        }
        tokio::select! {
            maybe_key = key_rx.recv() => {
                match maybe_key {
                    Some(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
                    Some(_) => {}
                    None => break Ok(()),
                }
            }
            maybe_event = event_rx.recv() => {
                if let Some(engine_event) = maybe_event {
                    app.on_event(engine_event);
                }
            }
            _ = tokio::time::sleep(spinner_interval) => {
                // Re-draw to advance the spinner animation.
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}

/// Load the `.env` file, walking up from CWD. When running as root, also
/// tries `$SUDO_USER`'s home so `sudo deathPWN` finds the config.
fn load_dotenv() {
    let _ = dotenvy::dotenv();

    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        if sudo_user != "root" {
            let home = format!("/home/{sudo_user}");
            // Try ~/.env, ~/.config/deathpwn/.env, and CWD's .env
            for path in &[
                format!("{home}/.config/deathpwn/.env"),
                format!("{home}/.env"),
            ] {
                let _ = dotenvy::from_path(std::path::PathBuf::from(path));
            }
        }
    }

    // Also try CWD's .env as last resort
    if let Ok(cwd) = std::env::current_dir() {
        let _ = dotenvy::from_path(cwd.join(".env"));
    }
}
