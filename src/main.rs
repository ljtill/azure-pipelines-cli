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
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use tokio::sync::mpsc;

use crate::api::client::AdoClient;
use crate::app::{App, AppMessage, TimelineRow};
use crate::config::Config;
use crate::events::{Action, handle_key};

#[derive(Parser)]
#[command(
    name = "azure-pipelines-cli",
    about = "TUI dashboard for Azure DevOps Pipelines"
)]
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

    let client = AdoClient::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
    )
    .await?;

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
    let mut app = App::new(
        &config.azure_devops.organization,
        &config.azure_devops.project,
        config,
    );
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
                if let Ok(Ok(Some(crossterm::event::Event::Key(key)))) = event_result
                    && key.kind == KeyEventKind::Press
                {
                    let action = handle_key(&mut app, key);
                    handle_action(&mut app, &client, &tx, action, &mut last_data_fetch);
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
                        let _ = tx
                            .send(AppMessage::Error(format!("Fetch builds: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::FetchTimeline(build_id) => {
            spawn_timeline_fetch(client, tx, build_id, app.log_generation, false);
        }
        Action::FetchBuildLog { build_id, log_id } => {
            spawn_log_fetch(client, tx, build_id, log_id, app.log_generation);
        }
        Action::FollowLatest => {
            // Switch to follow mode: jump cursor to active task and fetch its log
            if let Some((idx, log_id)) = app.auto_select_log_entry() {
                if let Some(TimelineRow::Task { name, .. }) = app.timeline_rows.get(idx) {
                    app.followed_task_name = name.clone();
                }
                app.followed_log_id = Some(log_id);
                if let Some(build) = &app.selected_build {
                    spawn_log_fetch(client, tx, build.id, log_id, app.log_generation);
                }
            }
        }
        Action::OpenInBrowser(url) => {
            // Fire-and-forget: open the URL in the default browser
            let _ = open_url(&url);
        }
        Action::CancelBuild(build_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.cancel_build(build_id).await {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::BuildCancelled).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Cancel build: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::CancelBuilds(build_ids) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut set = tokio::task::JoinSet::new();
                for &id in &build_ids {
                    let client = client.clone();
                    set.spawn(async move { client.cancel_build(id).await });
                }
                let mut cancelled = 0u32;
                let mut failed = 0u32;
                while let Some(result) = set.join_next().await {
                    match result {
                        Ok(Ok(())) => cancelled += 1,
                        _ => failed += 1,
                    }
                }
                let _ = tx
                    .send(AppMessage::BuildsCancelled { cancelled, failed })
                    .await;
            });
        }
        Action::RetryStage {
            build_id,
            stage_ref_name,
        } => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.retry_stage(build_id, &stage_ref_name).await {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::StageRetried).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Retry stage: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::QueuePipeline(definition_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client.run_pipeline(definition_id).await {
                    Ok(run) => {
                        // Fetch the full build object so we can navigate to it
                        match client.get_build(run.id).await {
                            Ok(build) => {
                                let _ = tx
                                    .send(AppMessage::PipelineQueued {
                                        build,
                                        definition_id,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                let _ = tx
                                    .send(AppMessage::Error(format!("Fetch queued build: {e}")))
                                    .await;
                            }
                        }
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Queue pipeline: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::ApproveCheck(approval_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client
                    .update_approval(&approval_id, "approved", "Approved via CLI")
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::CheckUpdated).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Approve check: {e}")))
                            .await;
                    }
                }
            });
        }
        Action::RejectCheck(approval_id) => {
            let client = client.clone();
            let tx = tx.clone();
            tokio::spawn(async move {
                match client
                    .update_approval(&approval_id, "rejected", "Rejected via CLI")
                    .await
                {
                    Ok(()) => {
                        let _ = tx.send(AppMessage::CheckUpdated).await;
                    }
                    Err(e) => {
                        let _ = tx
                            .send(AppMessage::Error(format!("Reject check: {e}")))
                            .await;
                    }
                }
            });
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
            pending_approvals,
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
            app.pending_approvals = pending_approvals;

            app.rebuild_dashboard_rows();
            app.rebuild_filtered_pipelines();
            app.rebuild_filtered_active_builds();
            app.last_refresh = Some(chrono::Utc::now());
            app.loading = false;
            app.error_message = None;
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
                    spawn_log_fetch(client, tx, build_id, log_id, app.log_generation);
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
                        spawn_log_fetch(client, tx, build_id, log_id, app.log_generation);
                    }
                    // If same task, spawn_log_refresh already handles content refresh
                }
            } else {
                // Refresh in inspect mode: only update tree status, preserve cursor + log
                app.rebuild_timeline_rows();
            }
        }
        AppMessage::LogContent {
            content,
            generation,
        } => {
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
        AppMessage::BuildCancelled => {
            // Trigger a data refresh to update build status
            spawn_data_refresh(client, tx);
            // If we're viewing this build's logs, refresh timeline too
            if let Some(build) = &app.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_generation, true);
            }
        }
        AppMessage::BuildsCancelled { cancelled, failed } => {
            app.selected_builds.clear();
            spawn_data_refresh(client, tx);
            if failed > 0 {
                app.error_message = Some(format!("Cancelled {cancelled}, {failed} failed"));
            }
        }
        AppMessage::StageRetried => {
            // Refresh timeline to show the retried stage
            if let Some(build) = &app.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_generation, true);
            }
            spawn_data_refresh(client, tx);
        }
        AppMessage::CheckUpdated => {
            // Refresh approvals + timeline
            spawn_data_refresh(client, tx);
            if let Some(build) = &app.selected_build {
                spawn_timeline_fetch(client, tx, build.id, app.log_generation, true);
            }
        }
        AppMessage::PipelineQueued {
            build,
            definition_id: _,
        } => {
            // Navigate to the new build's log viewer
            let build_id = build.id;
            app.navigate_to_log_viewer(build);
            // Fetch its timeline
            spawn_timeline_fetch(client, tx, build_id, app.log_generation, false);
        }
    }
}

fn spawn_log_fetch(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
    log_id: u32,
    generation: u64,
) {
    let client = client.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        match client.get_build_log(build_id, log_id).await {
            Ok(content) => {
                let _ = tx
                    .send(AppMessage::LogContent {
                        content,
                        generation,
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx.send(AppMessage::Error(format!("Fetch log: {e}"))).await;
            }
        }
    });
}

fn spawn_timeline_fetch(
    client: &AdoClient,
    tx: &mpsc::Sender<AppMessage>,
    build_id: u32,
    generation: u64,
    is_refresh: bool,
) {
    let client = client.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        match client.get_build_timeline(build_id).await {
            Ok(timeline) => {
                let _ = tx
                    .send(AppMessage::Timeline {
                        build_id,
                        timeline,
                        generation,
                        is_refresh,
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

fn spawn_data_refresh(client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let client = client.clone();
    let tx = tx.clone();
    tokio::spawn(async move {
        let (defs_result, recent_result, active_result, approvals_result) = tokio::join!(
            client.list_definitions(),
            client.list_recent_builds(),
            client.list_active_builds(),
            client.list_pending_approvals(),
        );

        let pending_approvals = approvals_result.unwrap_or_default();

        match (defs_result, recent_result, active_result) {
            (Ok(definitions), Ok(recent_builds), Ok(active_builds)) => {
                let _ = tx
                    .send(AppMessage::DataRefresh {
                        definitions,
                        recent_builds,
                        active_builds,
                        pending_approvals,
                    })
                    .await;
            }
            (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                let _ = tx.send(AppMessage::Error(format!("Refresh: {e}"))).await;
            }
        }
    });
}

fn spawn_log_refresh(app: &App, client: &AdoClient, tx: &mpsc::Sender<AppMessage>) {
    let generation = app.log_generation;

    // Re-fetch timeline for in-progress builds
    if let Some(build) = &app.selected_build
        && build.status.is_in_progress()
    {
        spawn_timeline_fetch(client, tx, build.id, generation, true);
    }

    // Re-fetch log content for the currently viewed task.
    // In follow mode: refresh the followed task's log.
    // In inspect mode: refresh the selected (pinned) task's log.
    let log_id_to_refresh = if app.follow_mode {
        app.followed_log_id
    } else {
        app.timeline_task_log_id(app.log_entries_index)
    };

    if !app.log_content.is_empty()
        && let Some(build) = &app.selected_build
        && let Some(log_id) = log_id_to_refresh
    {
        let client = client.clone();
        let tx = tx.clone();
        let build_id = build.id;
        tokio::spawn(async move {
            if let Ok(content) = client.get_build_log(build_id, log_id).await {
                let _ = tx
                    .send(AppMessage::LogContent {
                        content,
                        generation,
                    })
                    .await;
            }
        });
    }
}

/// Open a URL in the platform's default browser.
fn open_url(url: &str) -> std::io::Result<std::process::Child> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "unsupported platform",
        ))
    }
}
