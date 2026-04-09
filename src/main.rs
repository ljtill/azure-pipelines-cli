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
use crossterm::event::Event;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use crossterm::{execute, event::KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::api::client::AdoClient;
use crate::app::App;
use crate::config::Config;
use crate::events::{Action, handle_key, poll_event};

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

    let result = run(&mut terminal, &client, &config).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    client: &AdoClient,
    config: &Config,
) -> Result<()> {
    let mut app = App::new();
    let refresh_interval = Duration::from_secs(config.display.refresh_interval_secs);
    let log_refresh_interval = Duration::from_secs(config.display.log_refresh_interval_secs);
    let mut last_data_fetch = Instant::now() - refresh_interval; // trigger immediate fetch
    let mut last_log_fetch: Option<Instant> = None;

    loop {
        if !app.running {
            break;
        }

        // Auto-refresh data
        let should_refresh_data = last_data_fetch.elapsed() >= refresh_interval;
        let should_refresh_logs = app.view == app::View::LogViewer
            && app.selected_build.is_some()
            && last_log_fetch
                .map(|t| t.elapsed() >= log_refresh_interval)
                .unwrap_or(true);

        if should_refresh_data {
            fetch_main_data(&mut app, client).await;
            last_data_fetch = Instant::now();
        }

        if should_refresh_logs {
            refresh_log_data(&mut app, client).await;
            last_log_fetch = Some(Instant::now());
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

        // Poll for events (short timeout to keep UI responsive)
        if let Some(event) = poll_event(Duration::from_millis(250))? {
            if let Event::Key(key) = event {
                // crossterm sends press + release on some platforms; only handle press
                if key.kind == KeyEventKind::Press {
                    let action = handle_key(&mut app, key);
                    match action {
                        Action::Quit => app.running = false,
                        Action::ForceRefresh => {
                            fetch_main_data(&mut app, client).await;
                            last_data_fetch = Instant::now();
                        }
                        Action::FetchBuildHistory(def_id) => {
                            match client.list_builds_for_definition(def_id).await {
                                Ok(builds) => app.definition_builds = builds,
                                Err(e) => app.error_message = Some(format!("Fetch builds: {e}")),
                            }
                        }
                        Action::FetchTimeline(build_id) => {
                            match client.get_build_timeline(build_id).await {
                                Ok(timeline) => {
                                    app.build_timeline = Some(timeline);
                                    app.log_content.clear();
                                    app.log_entries_index = 0;

                                    // Auto-select and fetch the most relevant log
                                    if let Some((_idx, log_id)) = app.auto_select_log_entry() {
                                        match client.get_build_log(build_id, log_id).await {
                                            Ok(content) => {
                                                app.log_content =
                                                    content.lines().map(String::from).collect();
                                                app.log_auto_scroll = true;
                                                app.log_scroll_offset = 0;
                                            }
                                            Err(e) => {
                                                app.error_message =
                                                    Some(format!("Fetch log: {e}"))
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.error_message = Some(format!("Fetch timeline: {e}"))
                                }
                            }
                        }
                        Action::FetchBuildLog { build_id, log_id } => {
                            match client.get_build_log(build_id, log_id).await {
                                Ok(content) => {
                                    app.log_content =
                                        content.lines().map(String::from).collect();
                                    app.log_auto_scroll = true;
                                    app.log_scroll_offset = 0;
                                }
                                Err(e) => {
                                    app.error_message = Some(format!("Fetch log: {e}"))
                                }
                            }
                        }
                        Action::None => {}
                    }
                }
            }
        }
    }

    Ok(())
}

async fn fetch_main_data(app: &mut App, client: &AdoClient) {
    app.loading = true;
    app.error_message = None;

    // Fetch definitions
    match client.list_definitions().await {
        Ok(defs) => app.definitions = defs,
        Err(e) => {
            app.error_message = Some(format!("Definitions: {e}"));
            app.loading = false;
            return;
        }
    }

    // Fetch recent builds and active builds
    let (recent_result, active_result) = tokio::join!(
        client.list_recent_builds(),
        client.list_active_builds(),
    );

    match recent_result {
        Ok(builds) => {
            // Build a map of latest build per definition
            let mut map: BTreeMap<u32, api::models::Build> = BTreeMap::new();
            for build in &builds {
                map.entry(build.definition.id)
                    .or_insert_with(|| build.clone());
            }
            app.latest_builds_by_def = map;
            app.recent_builds = builds;
        }
        Err(e) => app.error_message = Some(format!("Builds: {e}")),
    }

    match active_result {
        Ok(builds) => app.active_builds = builds,
        Err(e) => {
            app.error_message =
                Some(app.error_message.take().unwrap_or_default() + &format!(" Active: {e}"))
        }
    }

    app.rebuild_dashboard_rows();
    app.rebuild_filtered_pipelines();
    app.last_refresh = Some(chrono::Utc::now());
    app.loading = false;
}

async fn refresh_log_data(app: &mut App, client: &AdoClient) {
    // Re-fetch timeline for the current build (to update step states)
    if let Some(build) = &app.selected_build {
        if build.status == "inProgress" || build.status == "InProgress" {
            if let Ok(timeline) = client.get_build_timeline(build.id).await {
                app.build_timeline = Some(timeline);
            }
        }
    }

    // Re-fetch the currently viewed log content
    if !app.log_content.is_empty() {
        if let Some(build) = &app.selected_build {
            if let Some(timeline) = &app.build_timeline {
                let log_records: Vec<_> = timeline
                    .records
                    .iter()
                    .filter(|r| r.log.is_some())
                    .collect();
                if let Some(record) = log_records.get(app.log_entries_index) {
                    if let Some(log_ref) = &record.log {
                        if let Ok(content) = client.get_build_log(build.id, log_ref.id).await {
                            app.log_content = content.lines().map(String::from).collect();
                        }
                    }
                }
            }
        }
    }
}

