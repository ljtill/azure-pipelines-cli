mod api;
mod app;
mod config;
mod events;
mod ui;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::event::KeyEventKind;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::app::{App, AppMessage, TimelineRow};
use crate::config::Config;
use crate::events::{Action, handle_key};

#[derive(Parser)]
#[command(name = "pipelines-dashboard", about = "TUI dashboard for Azure DevOps Pipelines")]
struct Cli {
    /// Path to config file
    #[arg(short, long)]
    config: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Panic hook to restore terminal
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stderr(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));

    let cli = Cli::parse();
    let config = Config::load(cli.config.as_ref())?;

    let client = AdoClient::new(&config.azure_devops.organization, &config.azure_devops.project).await?;

    // Terminal setup
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, client, &config).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    client: AdoClient,
    config: &Config,
) -> Result<()> {
    let mut app = App::new();
    let refresh_interval = Duration::from_secs(config.display.refresh_interval_secs);
    let log_refresh_interval = Duration::from_secs(config.display.log_refresh_interval_secs);
    let mut last_data_fetch = Instant::now() - refresh_interval; // trigger immediate fetch
    let mut last_log_fetch: Option<Instant> = None;

    let (tx, mut rx) = mpsc::channel::<AppMessage>(64);

    loop {
        if !app.running {
            break;
        }

        // Spawn periodic background refreshes
        let should_refresh_data = last_data_fetch.elapsed() >= refresh_interval;
        let should_refresh_logs = app.view == app::View::LogViewer
            && app.selected_build.is_some()
            && last_log_fetch
                .map(|t| t.elapsed() >= log_refresh_interval)
                .unwrap_or(true);

        if should_refresh_data {
            spawn_data_refresh(&client, &tx);
            last_data_fetch = Instant::now();
        }

        if should_refresh_logs {
            spawn_log_refresh(&app, &client, &tx);
            last_log_fetch = Some(Instant::now());
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll terminal events (non-blocking via spawn_blocking) and channel concurrently
        tokio::select! {
            // Terminal event
            event_result = tokio::task::spawn_blocking(|| {
                events::poll_event(Duration::from_millis(100))
            }) => {
                if let Ok(Ok(Some(event))) = event_result {
                    if let crossterm::event::Event::Key(key) = event {
                        if key.kind == KeyEventKind::Press {
                            let action = handle_key(&mut app, key);
                            handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
                        }
                    }
                }
            }

            // Background task result
            Some(msg) = rx.recv() => {
                handle_message(&mut app, &client, &tx, msg);
            }
        }
    }

    Ok(())
}

fn handle_action(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    action: Action,
    last_data_fetch: &mut Instant,
) {
    match action {
        Action::Quit => app.running = false,
        Action::ForceRefresh => {
            spawn_data_refresh(client, tx);
            *last_data_fetch = Instant::now();
        }
        Action::FetchBuildHistory(def_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.list_builds_for_definition(def_id).await {
                    Ok(builds) => {
                        let _ = tx.send(AppMessage::BuildHistory { builds }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(AppMessage::Error(format!("Fetch builds: {e}"))).await;
                    }
                }
            });
        }
        Action::FetchTimeline(build_id) => {
            let client = client.clone();
            let tx = tx.clone();
            let generation = app.log_generation;
            tokio::spawn(async move {
                match client.get_build_timeline(build_id).await {
                    Ok(timeline) => {
                        let _ = tx
                            .send(AppMessage::Timeline {
                                build_id,
                                timeline,
                                generation,
                                is_refresh: false,
                            })
                            .await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Fetch timeline: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::FetchBuildLog { build_id, log_id } => {
            let client = client.clone();
            let tx = tx.clone();
            let generation = app.log_generation;
            tokio::spawn(async move {
                match client.get_build_log(build_id, log_id).await {
                    Ok(content) => {
                        let _ = tx.send(AppMessage::LogContent { content, generation }).await;
                    }
                    Err(e) => {
                        let _ = tx.send(AppMessage::Error(format!("Fetch log: {e}"))).await;
                    }
                }
            });
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log
            if let Some((idx, log_id)) = app.auto_select_log_entry() {
                if let Some(TimelineRow::Task { name, .. }) = app.timeline_rows.get(idx) {
                    app.followed_task_name = name.clone();
                }
                app.followed_log_id = Some(log_id);
                if let Some(build) = &app.selected_build {
                    let client = client.clone();
                    let tx = tx.clone();
                    let generation = app.log_generation;
                    let build_id = build.id;
                    tokio::spawn(async move {
                        match client.get_build_log(build_id, log_id).await {
                            Ok(content) => {
                                let _ = tx
                                    .send(AppMessage::LogContent { content, generation })
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(AppMessage::Error(format!("Fetch log: {e}")))
                                    .await;
                            }
                        }
                    });
                }
            }
        }
        Action::None => {}
    }
}

fn handle_message(
    app: &mut App,
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    msg: AppMessage,
) {
    match msg {
        AppMessage::DataRefresh {
            definitions,
            recent_builds,
            active_builds,
        } => {
            app.definitions = definitions;

            let mut map: BTreeMap<u32, api::models::Build> = BTreeMap::new();
            for build in &recent_builds {
                map.entry(build.definition.id)
                    .or_insert_with(|| build.clone());
            }
            app.latest_builds_by_def = map;
            app.recent_builds = recent_builds;
            app.active_builds = active_builds;

            app.rebuild_dashboard_rows();
            app.rebuild_filtered_pipelines();
            app.last_refresh = Some(chrono::Utc::now());
            app.loading = false;
        }
        AppMessage::BuildHistory { builds } => {
            app.definition_builds = builds;
        }
        AppMessage::Timeline {
            build_id,
            timeline,
            generation,
            is_refresh,
        } => {
            // Discard stale timeline results
            if generation != app.log_generation {
                return;
            }

            app.build_timeline = Some(timeline);

            if !is_refresh {
                // Initial load: full setup with auto-select
                app.log_content.clear();
                app.log_entries_index = 0;
                app.follow_mode = true;
                app.rebuild_timeline_rows();

                if let Some((_idx, log_id)) = app.auto_select_log_entry() {
                    // Set follow tracking info
                    if let Some(TimelineRow::Task { name, .. }) =
                        app.timeline_rows.get(app.log_entries_index)
                    {
                        app.followed_task_name = name.clone();
                    }
                    app.followed_log_id = Some(log_id);

                    let client = client.clone();
                    let tx = tx.clone();
                    let generation = app.log_generation;
                    tokio::spawn(async move {
                        match client.get_build_log(build_id, log_id).await {
                            Ok(content) => {
                                let _ = tx
                                    .send(AppMessage::LogContent { content, generation })
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(AppMessage::Error(format!("Fetch log: {e}")))
                                    .await;
                            }
                        }
                    });
                }
            } else if app.follow_mode {
                // Refresh in follow mode: update tree, track latest active task
                app.rebuild_timeline_rows();

                if let Some((task_name, log_id)) = app.find_active_task() {
                    let task_changed = app.followed_log_id != Some(log_id);
                    app.followed_task_name = task_name;
                    app.followed_log_id = Some(log_id);

                    if task_changed {
                        // Active task changed — fetch the new task's log
                        let client = client.clone();
                        let tx = tx.clone();
                        let generation = app.log_generation;
                        tokio::spawn(async move {
                            match client.get_build_log(build_id, log_id).await {
                                Ok(content) => {
                                    let _ = tx
                                        .send(AppMessage::LogContent { content, generation })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = tx
                                        .send(AppMessage::Error(format!("Fetch log: {e}")))
                                        .await;
                                }
                            }
                        });
                    }
                    // If same task, spawn_log_refresh already handles content refresh
                }
            } else {
                // Refresh in inspect mode: only update tree status, preserve cursor + log
                app.rebuild_timeline_rows();
            }
        }
        AppMessage::LogContent { content, generation } => {
            // Discard stale log results
            if generation != app.log_generation {
                return;
            }
            app.log_content = content.lines().map(String::from).collect();
            app.log_auto_scroll = true;
            app.log_scroll_offset = 0;
        }
        AppMessage::Error(msg) => {
            app.error_message = Some(msg);
        }
    }
}

fn spawn_data_refresh(client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let client = client.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let (defs_result, recent_result, active_result) = tokio::join!(
            client.list_definitions(),
            client.list_recent_builds(),
            client.list_active_builds(),
        );

        match (defs_result, recent_result, active_result) {
            (Ok(definitions), Ok(recent_builds), Ok(active_builds)) => {
                let _ = tx
                    .send(AppMessage::DataRefresh {
                        definitions,
                        recent_builds,
                        active_builds,
                    })
                    .await;
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                let _ = tx
                    .send(AppMessage::Error(format!("Refresh: {e}")))
                    .await;
            }
        }
    });
}

fn spawn_log_refresh(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let generation = app.log_generation;

    // Re-fetch timeline for in-progress builds
    if let Some(build) = &app.selected_build {
        if build.status == "inProgress" || build.status == "InProgress" {
            let client = client.clone();
            let tx = tx.clone();
            let build_id = build.id;
            tokio::spawn(async move {
                if let Ok(timeline) = client.get_build_timeline(build_id).await {
                    let _ = tx
                        .send(AppMessage::Timeline {
                            build_id,
                            timeline,
                            generation,
                            is_refresh: true,
                        })
                        .await;
                }
            });
        }
    }

    // Re-fetch log content for the currently viewed task.
    // In follow mode: refresh the followed task's log.
    // In inspect mode: refresh the selected (pinned) task's log.
    let log_id_to_refresh = if app.follow_mode {
        app.followed_log_id
    } else {
        app.timeline_task_log_id(app.log_entries_index)
    };

    if !app.log_content.is_empty() {
        if let Some(build) = &app.selected_build {
            if let Some(log_id) = log_id_to_refresh {
                let client = client.clone();
                let tx = tx.clone();
                let build_id = build.id;
                tokio::spawn(async move {
                    if let Ok(content) = client.get_build_log(build_id, log_id).await {
                        let _ = tx.send(AppMessage::LogContent { content, generation }).await;
                    }
                });
            }
        }
    }
}

