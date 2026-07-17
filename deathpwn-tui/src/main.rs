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

use deathpwn_core::cache::PlanCache;
use deathpwn_core::clock::{Clock, SystemClock};
use deathpwn_core::config::Config;
use deathpwn_core::detector::Detector;
use deathpwn_core::engine::{Engine, EngineEvent};
use deathpwn_core::error::Result;
use deathpwn_core::exec::{FeedbackLoop, ShellRunner};
use deathpwn_core::pipeline::{Plan, Render, Retrieve, Understand};
use deathpwn_core::providers::{AiProvider, FailoverClient, OpenAiClient};
use deathpwn_core::search::{DuckDuckGoSearch, SearchProvider};
use deathpwn_core::session::{Artifacts, SessionState};

mod app;
mod ui;

use app::{App, Job, StatusBar};

#[tokio::main]
async fn main() -> Result<()> {
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
    let search: Arc<dyn SearchProvider> =
        Arc::new(DuckDuckGoSearch::with_timeout_secs(config.http_timeout_secs)?);

    let detector = Detector::new(ShellRunner::new(config.shell.clone()), config.shell.clone());
    let understand = Understand::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let retrieve = Retrieve::new(
        FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone()),
        search.clone(),
    );
    let plan = Plan::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let render = Render::new(FailoverClient::new(
        provider_a.clone(),
        provider_b.clone(),
        clock.clone(),
    ));
    let feedback = FeedbackLoop::new(
        ShellRunner::new(config.shell.clone()),
        provider_a.clone(),
        config.max_corrections,
    );
    let engine_ai =
        FailoverClient::new(provider_a.clone(), provider_b.clone(), clock.clone());

    let session = SessionState::new();
    let cache = PlanCache::new();
    let artifacts = Artifacts::open(config.artifacts_dir.clone(), clock.as_ref())?;

    let mut engine = Engine::new(
        detector, understand, retrieve, plan, render, feedback, session, cache, artifacts,
        engine_ai, config,
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

    let result: Result<()> = loop {
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
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    result
}
