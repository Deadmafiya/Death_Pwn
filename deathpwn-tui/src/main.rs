//! deathpwn-tui: the ratatui front end. Owns the tokio runtime, the terminal,
//! and all rendering. No business logic — it plumbs crossterm key events into
//! the core `Engine` and draws the `EngineEvent`s streamed back.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event, KeyEventKind};
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

    // Check if we have standard flags, but check for query args:
    let mut query_args = Vec::new();
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--history" {
            i += 2;
        } else if args[i].starts_with("--history=") {
            i += 1;
        } else if args[i] == "--no-cache"
            || args[i] == "--disable-cache"
            || args[i] == "--cache"
            || args[i] == "--enable-cache"
            || args[i] == "--clear-history"
        {
            i += 1;
        } else if args[i] == "-h" || args[i] == "--help" {
            println!("deathPWN - Agentic AI Coding Assistant / Offensive-Security Terminal");
            println!();
            println!("Usage: deathPWN [OPTIONS] [RAW_QUERY]");
            println!();
            println!("Options:");
            println!("  --no-cache, --disable-cache  Disable in-memory command caching");
            println!("  --cache, --enable-cache      Enable in-memory command caching");
            println!("  --clear-history              Clear all previous session command logs/history");
            println!("  -h, --help                   Print help information");
            println!();
            println!("If [RAW_QUERY] is provided, deathPWN resolves and executes the command directly in the terminal.");
            return Ok(());
        } else {
            query_args.push(args[i].clone());
            i += 1;
        }
    }

    if args
        .iter()
        .any(|arg| arg == "--no-cache" || arg == "--disable-cache")
    {
        std::env::set_var("DEATHPWN_DISABLE_CACHE", "true");
    } else if args
        .iter()
        .any(|arg| arg == "--cache" || arg == "--enable-cache")
    {
        std::env::set_var("DEATHPWN_DISABLE_CACHE", "false");
    }

    let mut history_val = None;
    let mut j = 0;
    while j < args.len() {
        if args[j] == "--history" {
            if j + 1 < args.len() {
                history_val = Some(args[j + 1].clone());
                j += 2;
            } else {
                eprintln!("Error: --history requires an argument (on/off/clear)");
                std::process::exit(1);
            }
        } else if args[j].starts_with("--history=") {
            let val = args[j].split_at("--history=".len()).1.to_string();
            history_val = Some(val);
            j += 1;
        } else {
            j += 1;
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
                eprintln!(
                    "Error: invalid value for --history: '{}'. Expected 'on', 'off', or 'clear'.",
                    val
                );
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
                eprintln!(
                    "Error clearing history directory '{}': {}",
                    artifacts_dir.display(),
                    e
                );
                std::process::exit(1);
            } else {
                println!("Cleared history directory: {}", artifacts_dir.display());
            }
        } else {
            println!("History directory does not exist or is already empty.");
        }
        return Ok(());
    }

    if !query_args.is_empty() {
        let query = query_args.join(" ");
        run_cli(&query).await?;
        return Ok(());
    }
    let (crossterm_tx, mut crossterm_rx) = mpsc::channel::<Event>(64);

    thread::spawn(move || loop {
        match event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                if let Ok(evt) = event::read() {
                    if crossterm_tx.blocking_send(evt).is_err() {
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
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    #[allow(unused_assignments)]
    let mut result: deathpwn_core::error::Result<()> = Ok(());

    loop {
        load_dotenv();
        let config = Config::from_env()?;
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
        let engine_ai = FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone());

        let runner = ShellRunner::new(config.shell.clone());
        let runner_clone = runner.clone();
        let mut engine = Engine::new(
            runner,
            engine_ai,
            config.preferences.clone(),
            config.shell.clone(),
        );

        let (job_tx, mut job_rx) = mpsc::channel::<Job>(64);
        let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(1024);
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(1024);

        tokio::spawn(async move {
            while let Some(job) = job_rx.recv().await {
                let _ = engine
                    .handle_line(&job.line, job.resolve_only, event_tx.clone(), job.cancel)
                    .await;
            }
        });

        tokio::spawn(async move {
            use deathpwn_core::exec::CommandRunner;
            while let Some(input) = stdin_rx.recv().await {
                let _ = runner_clone.write_stdin(&input).await;
            }
        });

        let mut app = App::new(job_tx, stdin_tx, StatusBar::new(provider_label));

        let spinner_interval = tokio::time::Duration::from_millis(80);

        result = loop {
            app.status.tick();
            if let Err(e) = terminal.draw(|f| ui::draw(f, &mut app)) {
                break Err(e.into());
            }
            if app.should_quit {
                break Ok(());
            }
            if app.should_reload {
                app.cancel.cancel();
                notify_ghostty("deathPWN reloading...");

                disable_raw_mode().ok();
                execute!(
                    terminal.backend_mut(),
                    LeaveAlternateScreen,
                    crossterm::event::DisableMouseCapture
                ).ok();
                terminal.show_cursor().ok();

                let exe = std::env::current_exe()
                    .unwrap_or_else(|_| std::path::PathBuf::from("deathPWN"));
                use std::os::unix::process::CommandExt as _;
                let err = std::process::Command::new(&exe)
                    .args(std::env::args().skip(1))
                    .exec();
                eprintln!("Failed to exec {:?}: {}", exe, err);
                std::process::exit(1);
            }
            tokio::select! {
                maybe_evt = crossterm_rx.recv() => {
                    match maybe_evt {
                        Some(Event::Key(key)) if key.kind == KeyEventKind::Press => app.handle_key(key),
                        Some(Event::Mouse(mouse)) => app.handle_mouse(mouse),
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

        if result.is_err() || app.should_quit {
            break;
        }
    }

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

fn load_env_file(path: &std::path::Path) {
    if let Ok(iter) = dotenvy::from_path_iter(path) {
        for item in iter {
            if let Ok((key, val)) = item {
                std::env::set_var(key, val);
            }
        }
    }
}

/// Load the `.env` file, walking up from CWD. When running as root, also
/// tries `$SUDO_USER`'s home so `sudo deathPWN` finds the config.
fn load_dotenv() {
    if let Ok(path) = dotenvy::dotenv() {
        load_env_file(&path);
    }

    if let Ok(sudo_user) = std::env::var("SUDO_USER") {
        if sudo_user != "root" {
            let home = format!("/home/{sudo_user}");
            // Try ~/.env, ~/.config/deathpwn/.env, and CWD's .env
            for path in &[
                format!("{home}/.config/deathpwn/.env"),
                format!("{home}/.env"),
            ] {
                load_env_file(std::path::Path::new(path));
            }
        }
    }

    // Also try CWD's .env as last resort
    if let Ok(cwd) = std::env::current_dir() {
        load_env_file(&cwd.join(".env"));
    }
}

fn notify_ghostty(message: &str) {
    use std::io::Write;
    let osc9 = format!("\x1b]9;{}\x07", message);
    let _ = std::io::stdout().write_all(osc9.as_bytes());
    let _ = std::io::stdout().flush();
}

async fn run_cli(query: &str) -> Result<()> {
    load_dotenv();
    let config = Config::from_env()?;

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
    let engine_ai = FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone());

    let runner = ShellRunner::new(config.shell.clone());
    let mut engine = Engine::new(
        runner,
        engine_ai,
        config.preferences.clone(),
        config.shell.clone(),
    );

    let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(1024);
    let cancel = deathpwn_core::cancel::CancelToken::new();

    println!("\x1b[1;36m[+] Resolving query via AI:\x1b[0m {}", query);

    let cancel_clone = cancel.clone();
    let query_string = query.to_string();
    tokio::spawn(async move {
        let _ = engine.handle_line(&query_string, true, event_tx, cancel_clone).await;
    });

    let mut resolved_command = None;
    while let Some(event) = event_rx.recv().await {
        match event {
            EngineEvent::Resolved(cmd) => {
                resolved_command = Some(cmd);
            }
            EngineEvent::Error(err) => {
                eprintln!("\x1b[1;31m[-] Resolution error:\x1b[0m {}", err);
            }
            _ => {}
        }
    }

    if let Some(cmd) = resolved_command {
        println!("\x1b[1;32m[+] Resolved command:\x1b[0m {}", cmd);
        println!("\x1b[1;33m[+] Executing...\x1b[0m");

        let (output_tx, mut output_rx) = mpsc::channel::<deathpwn_core::exec::OutputLine>(1024);
        let runner_exec = ShellRunner::new(config.shell.clone());

        let parsed_tokens = shell_words::split(&cmd).unwrap_or_else(|_| vec![cmd.clone()]);
        let mut iter = parsed_tokens.into_iter();
        if let Some(tool) = iter.next() {
            let spec = deathpwn_core::exec::CommandSpec {
                tool,
                argv: iter.collect(),
            };

            let printer_task = tokio::spawn(async move {
                while let Some(line) = output_rx.recv().await {
                    match line.stream {
                        deathpwn_core::exec::Stream::Stdout => {
                            println!("{}", line.text);
                        }
                        deathpwn_core::exec::Stream::Stderr => {
                            eprintln!("{}", line.text);
                        }
                        deathpwn_core::exec::Stream::Banner => {
                            println!("\x1b[1;35m{}\x1b[0m", line.text);
                        }
                    }
                }
            });

            let cancel_exec = deathpwn_core::cancel::CancelToken::new();
            use deathpwn_core::exec::CommandRunner;
            let outcome = runner_exec.run_streaming(&spec, output_tx, cancel_exec).await;
            let _ = printer_task.await;

            if let Some(exit_code) = outcome.exit {
                if exit_code != 0 {
                    std::process::exit(exit_code);
                }
            }
        }
    } else {
        eprintln!("\x1b[1;31m[-] Could not resolve command.\x1b[0m");
        std::process::exit(1);
    }

    Ok(())
}
