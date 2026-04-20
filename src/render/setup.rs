//! First-run setup wizard for initial configuration.

use std::path::PathBuf;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph};

use crate::config::Config;

// --- State ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Organization,
    Project,
}

pub struct SetupState {
    active: Field,
    pub organization: String,
    pub project: String,
}

impl SetupState {
    fn new() -> Self {
        Self {
            active: Field::Organization,
            organization: String::new(),
            project: String::new(),
        }
    }

    fn active_buffer_mut(&mut self) -> &mut String {
        match self.active {
            Field::Organization => &mut self.organization,
            Field::Project => &mut self.project,
        }
    }
}

/// Represents the outcome of processing a single key event.
enum Outcome {
    Continue,
    Complete,
    Quit,
}

// --- Event handling ---

fn handle_key(state: &mut SetupState, code: KeyCode) -> Outcome {
    match code {
        KeyCode::Esc => Outcome::Quit,
        KeyCode::Enter => match state.active {
            Field::Organization => {
                if !state.organization.trim().is_empty() {
                    state.active = Field::Project;
                }
                Outcome::Continue
            }
            Field::Project => {
                if state.project.trim().is_empty() {
                    Outcome::Continue
                } else {
                    Outcome::Complete
                }
            }
        },
        KeyCode::Tab => {
            state.active = match state.active {
                Field::Organization => Field::Project,
                Field::Project => Field::Organization,
            };
            Outcome::Continue
        }
        KeyCode::Backspace => {
            state.active_buffer_mut().pop();
            Outcome::Continue
        }
        KeyCode::Char(c) => {
            state.active_buffer_mut().push(c);
            Outcome::Continue
        }
        _ => Outcome::Continue,
    }
}

// --- UI ---

fn draw(f: &mut Frame, state: &SetupState) {
    let dialog_width = 54;
    let dialog_height = 10;
    let area = centered_rect(dialog_width, dialog_height, f.area());

    f.render_widget(Clear, area);

    let block = Block::bordered()
        .title(" Welcome to devops ")
        .title_alignment(Alignment::Center)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let rows = Layout::vertical([
        Constraint::Length(1), // Subtitle.
        Constraint::Length(1), // Blank.
        Constraint::Length(1), // Org label + field.
        Constraint::Length(1), // Project label + field.
        Constraint::Length(1), // Blank.
        Constraint::Length(1), // Hints.
    ])
    .split(inner);

    // Subtitle.
    let subtitle = Paragraph::new("No configuration found. Let's set things up.")
        .style(Style::new().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(subtitle, rows[0]);

    // Fields.
    draw_field(
        f,
        rows[2],
        "Organization",
        &state.organization,
        state.active == Field::Organization,
    );
    draw_field(
        f,
        rows[3],
        "Project     ",
        &state.project,
        state.active == Field::Project,
    );

    // Hints.
    let hints = Paragraph::new(Line::from(vec![
        Span::styled(
            "Enter",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" next  "),
        Span::styled(
            "Tab",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" switch  "),
        Span::styled(
            "Esc",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" quit"),
    ]))
    .alignment(Alignment::Center);
    f.render_widget(hints, rows[5]);
}

fn draw_field(f: &mut Frame, area: Rect, label: &str, value: &str, active: bool) {
    let style = if active {
        Style::new().fg(Color::Cyan)
    } else {
        Style::new().fg(Color::DarkGray)
    };

    let cursor = if active { "█" } else { "" };
    let line = Line::from(vec![
        Span::styled(format!("  {label}: "), style.add_modifier(Modifier::BOLD)),
        Span::styled(value, Style::new().fg(Color::White)),
        Span::styled(cursor, Style::new().fg(Color::Cyan)),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}

// --- Public entry point ---

/// Runs the interactive setup flow. Returns `Ok(Some(config))` on success,
/// `Ok(None)` if the user pressed Esc to quit.
pub async fn run_setup(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    config_path: &PathBuf,
) -> Result<Option<Config>> {
    tracing::info!(path = %config_path.display(), "starting setup wizard");
    let mut state = SetupState::new();

    loop {
        terminal.draw(|f| draw(f, &state))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match handle_key(&mut state, key.code) {
                Outcome::Continue => {}
                Outcome::Quit => {
                    tracing::debug!("setup wizard cancelled by user");
                    return Ok(None);
                }
                Outcome::Complete => {
                    let org = state.organization.trim().to_string();
                    let proj = state.project.trim().to_string();
                    tracing::info!(
                        organization = &*org,
                        project = &*proj,
                        "setup wizard complete"
                    );
                    Config::write_initial(config_path, &org, &proj).await?;
                    let config = Config::load(Some(config_path)).await?;
                    return Ok(Some(config));
                }
            }
        }
    }
}

// --- Tests ---

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let s = SetupState::new();
        assert_eq!(s.active, Field::Organization);
        assert!(s.organization.is_empty());
        assert!(s.project.is_empty());
    }

    #[test]
    fn typing_appends_to_active_field() {
        let mut s = SetupState::new();
        handle_key(&mut s, KeyCode::Char('a'));
        handle_key(&mut s, KeyCode::Char('b'));
        assert_eq!(s.organization, "ab");
        assert!(s.project.is_empty());
    }

    #[test]
    fn backspace_removes_last_char() {
        let mut s = SetupState::new();
        handle_key(&mut s, KeyCode::Char('x'));
        handle_key(&mut s, KeyCode::Char('y'));
        handle_key(&mut s, KeyCode::Backspace);
        assert_eq!(s.organization, "x");
    }

    #[test]
    fn enter_advances_from_org_to_project() {
        let mut s = SetupState::new();
        handle_key(&mut s, KeyCode::Char('o'));
        let outcome = handle_key(&mut s, KeyCode::Enter);
        assert!(matches!(outcome, Outcome::Continue));
        assert_eq!(s.active, Field::Project);
    }

    #[test]
    fn enter_on_empty_org_does_not_advance() {
        let mut s = SetupState::new();
        handle_key(&mut s, KeyCode::Enter);
        assert_eq!(s.active, Field::Organization);
    }

    #[test]
    fn enter_on_project_completes() {
        let mut s = SetupState::new();
        s.active = Field::Project;
        handle_key(&mut s, KeyCode::Char('p'));
        let outcome = handle_key(&mut s, KeyCode::Enter);
        assert!(matches!(outcome, Outcome::Complete));
    }

    #[test]
    fn enter_on_empty_project_does_not_complete() {
        let mut s = SetupState::new();
        s.active = Field::Project;
        let outcome = handle_key(&mut s, KeyCode::Enter);
        assert!(matches!(outcome, Outcome::Continue));
        assert_eq!(s.active, Field::Project);
    }

    #[test]
    fn tab_switches_field() {
        let mut s = SetupState::new();
        handle_key(&mut s, KeyCode::Tab);
        assert_eq!(s.active, Field::Project);
        handle_key(&mut s, KeyCode::Tab);
        assert_eq!(s.active, Field::Organization);
    }

    #[test]
    fn esc_quits() {
        let mut s = SetupState::new();
        let outcome = handle_key(&mut s, KeyCode::Esc);
        assert!(matches!(outcome, Outcome::Quit));
    }

    #[tokio::test]
    async fn write_initial_creates_valid_config() {
        let dir = std::env::temp_dir().join("devops-test-write-config");
        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("config.toml");

        Config::write_initial(&path, "test-org", "test-proj")
            .await
            .unwrap();
        let config = Config::load(Some(&path)).await.unwrap();
        assert_eq!(config.azure_devops.organization, "test-org");
        assert_eq!(config.azure_devops.project, "test-proj");

        // Safe: test-only cleanup.
        let _ = std::fs::remove_dir_all(&dir);
    }
}
