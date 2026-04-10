use std::path::PathBuf;

use crate::config::{
    AzureDevOpsConfig, Config, DisplayConfig, FiltersConfig, LoggingConfig, NotificationsConfig,
    UpdateConfig,
};

/// Which field type a settings row represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// Free-form text input.
    Text,
    /// Comma-separated list of strings.
    StringList,
    /// Comma-separated list of u32 values.
    IntList,
    /// Boolean toggle (Enter/Space to flip).
    Toggle,
    /// Cycle through a fixed set of values (Enter to advance).
    Cycle,
    /// Positive integer input.
    Number,
}

/// A single row in the settings form.
#[derive(Debug, Clone)]
pub struct SettingsField {
    pub label: &'static str,
    pub section: &'static str,
    pub kind: FieldKind,
    /// Current value serialized to a display string.
    pub value: String,
    /// Hint text shown to the right of the field.
    pub hint: &'static str,
}

const LOG_LEVELS: &[&str] = &["trace", "debug", "info", "warn", "error"];

/// Editable settings state, populated from the current config on open.
#[derive(Debug)]
pub struct SettingsState {
    pub fields: Vec<SettingsField>,
    /// Index of the currently selected/focused field.
    pub selected: usize,
    /// Whether the currently selected field is in edit mode.
    pub editing: bool,
    /// The path to the config file (needed for save).
    pub config_path: PathBuf,
    /// Cursor position within the edit buffer.
    pub cursor: usize,
}

impl SettingsState {
    /// Build a new `SettingsState` from the running config and file path.
    pub fn from_config(config: &Config, config_path: PathBuf) -> Self {
        let fields = vec![
            // Connection
            SettingsField {
                label: "Organization",
                section: "Connection",
                kind: FieldKind::Text,
                value: config.azure_devops.organization.clone(),
                hint: "reload on save",
            },
            SettingsField {
                label: "Project",
                section: "Connection",
                kind: FieldKind::Text,
                value: config.azure_devops.project.clone(),
                hint: "reload on save",
            },
            // Filters
            SettingsField {
                label: "Filter folders",
                section: "Filters",
                kind: FieldKind::StringList,
                value: config.filters.folders.join(", "),
                hint: "comma-separated",
            },
            SettingsField {
                label: "Filter definition IDs",
                section: "Filters",
                kind: FieldKind::IntList,
                value: config
                    .filters
                    .definition_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                hint: "comma-separated",
            },
            // Display
            SettingsField {
                label: "Refresh interval (secs)",
                section: "Display",
                kind: FieldKind::Number,
                value: config.display.refresh_interval_secs.to_string(),
                hint: "min 5",
            },
            SettingsField {
                label: "Log refresh interval (secs)",
                section: "Display",
                kind: FieldKind::Number,
                value: config.display.log_refresh_interval_secs.to_string(),
                hint: "min 1",
            },
            SettingsField {
                label: "Notifications",
                section: "Display",
                kind: FieldKind::Toggle,
                value: config.notifications.enabled.to_string(),
                hint: "",
            },
            // General
            SettingsField {
                label: "Log level",
                section: "General",
                kind: FieldKind::Cycle,
                value: config.logging.level.clone(),
                hint: "trace/debug/info/warn/error",
            },
            SettingsField {
                label: "Check for updates",
                section: "General",
                kind: FieldKind::Toggle,
                value: config.update.check_for_updates.to_string(),
                hint: "",
            },
        ];
        Self {
            fields,
            selected: 0,
            editing: false,
            config_path,
            cursor: 0,
        }
    }

    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    pub fn move_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if self.selected + 1 < self.fields.len() {
            self.selected += 1;
        }
    }

    /// Enter edit mode for the current field.
    pub fn start_edit(&mut self) {
        let field = &self.fields[self.selected];
        match field.kind {
            FieldKind::Toggle => {
                // Toggle immediately, no edit mode needed.
                let current = self.fields[self.selected].value == "true";
                self.fields[self.selected].value = (!current).to_string();
            }
            FieldKind::Cycle => {
                // Advance to the next value in the cycle.
                let current = &self.fields[self.selected].value;
                let idx = LOG_LEVELS.iter().position(|&l| l == current).unwrap_or(0);
                let next = (idx + 1) % LOG_LEVELS.len();
                self.fields[self.selected].value = LOG_LEVELS[next].to_string();
            }
            _ => {
                self.editing = true;
                self.cursor = self.fields[self.selected].value.len();
            }
        }
    }

    /// Stop editing (confirm the current value).
    pub fn stop_edit(&mut self) {
        self.editing = false;
    }

    /// Cancel editing (we don't restore old value — Esc from the overlay
    /// discards the entire settings state, so individual field cancel
    /// isn't needed).
    pub fn cancel_edit(&mut self) {
        self.editing = false;
    }

    /// Insert a character at the cursor position for the active field.
    pub fn insert_char(&mut self, c: char) {
        let field = &mut self.fields[self.selected];
        match field.kind {
            FieldKind::Number => {
                if c.is_ascii_digit() {
                    field.value.insert(self.cursor, c);
                    self.cursor += 1;
                }
            }
            _ => {
                field.value.insert(self.cursor, c);
                self.cursor += 1;
            }
        }
    }

    /// Delete the character before the cursor.
    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.fields[self.selected].value.remove(self.cursor - 1);
            self.cursor -= 1;
        }
    }

    /// Delete the character at the cursor.
    pub fn delete(&mut self) {
        let len = self.fields[self.selected].value.len();
        if self.cursor < len {
            self.fields[self.selected].value.remove(self.cursor);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        let len = self.fields[self.selected].value.len();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    /// Build a `Config` from the current field values.
    pub fn to_config(&self) -> Config {
        let get = |label: &str| -> &str {
            self.fields
                .iter()
                .find(|f| f.label == label)
                .map(|f| f.value.as_str())
                .unwrap_or("")
        };

        let parse_bool = |label: &str| -> bool { get(label) == "true" };

        let parse_string_list = |label: &str| -> Vec<String> {
            let raw = get(label);
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                raw.split(',').map(|s| s.trim().to_string()).collect()
            }
        };

        let parse_u32_list = |label: &str| -> Vec<u32> {
            let raw = get(label);
            if raw.trim().is_empty() {
                Vec::new()
            } else {
                raw.split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect()
            }
        };

        let parse_u64 = |label: &str, default: u64| -> u64 {
            get(label).trim().parse::<u64>().unwrap_or(default)
        };

        Config {
            azure_devops: AzureDevOpsConfig {
                organization: get("Organization").to_string(),
                project: get("Project").to_string(),
            },
            filters: FiltersConfig {
                folders: parse_string_list("Filter folders"),
                definition_ids: parse_u32_list("Filter definition IDs"),
            },
            update: UpdateConfig {
                check_for_updates: parse_bool("Check for updates"),
            },
            logging: LoggingConfig {
                level: get("Log level").to_string(),
                log_directory: None,
                max_log_files: 5,
            },
            notifications: NotificationsConfig {
                enabled: parse_bool("Notifications"),
            },
            display: DisplayConfig {
                refresh_interval_secs: parse_u64("Refresh interval (secs)", 15).max(5),
                log_refresh_interval_secs: parse_u64("Log refresh interval (secs)", 5).max(1),
            },
        }
    }

    /// Save the settings to disk and return the built config.
    pub fn save(&self) -> Result<Config, anyhow::Error> {
        let config = self.to_config();
        config.save(&self.config_path)?;
        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::make_config;
    use std::path::PathBuf;

    fn make_settings() -> SettingsState {
        let config = make_config();
        SettingsState::from_config(&config, PathBuf::from("/tmp/test-config.toml"))
    }

    #[test]
    fn field_count() {
        let s = make_settings();
        assert_eq!(s.field_count(), 9);
    }

    #[test]
    fn navigate_up_down() {
        let mut s = make_settings();
        assert_eq!(s.selected, 0);
        s.move_down();
        assert_eq!(s.selected, 1);
        s.move_down();
        assert_eq!(s.selected, 2);
        s.move_up();
        assert_eq!(s.selected, 1);
        s.move_up();
        assert_eq!(s.selected, 0);
        // Can't go above 0
        s.move_up();
        assert_eq!(s.selected, 0);
    }

    #[test]
    fn navigate_down_clamps() {
        let mut s = make_settings();
        for _ in 0..20 {
            s.move_down();
        }
        assert_eq!(s.selected, s.field_count() - 1);
    }

    #[test]
    fn toggle_field() {
        let mut s = make_settings();
        // "Check for updates" is now index 8
        s.selected = 8;
        assert_eq!(s.fields[8].kind, FieldKind::Toggle);
        assert_eq!(s.fields[8].value, "true");
        s.start_edit();
        assert_eq!(s.fields[8].value, "false");
        assert!(!s.editing); // Toggle doesn't enter edit mode
        s.start_edit();
        assert_eq!(s.fields[8].value, "true");
    }

    #[test]
    fn cycle_field() {
        let mut s = make_settings();
        // "Log level" is now index 7
        s.selected = 7;
        assert_eq!(s.fields[7].kind, FieldKind::Cycle);
        assert_eq!(s.fields[7].value, "info");
        s.start_edit(); // info -> warn
        assert_eq!(s.fields[7].value, "warn");
        s.start_edit(); // warn -> error
        assert_eq!(s.fields[7].value, "error");
        s.start_edit(); // error -> trace (wraps)
        assert_eq!(s.fields[7].value, "trace");
    }

    #[test]
    fn text_edit_insert_and_backspace() {
        let mut s = make_settings();
        s.selected = 0; // Organization
        s.start_edit();
        assert!(s.editing);
        let len = s.fields[0].value.len();
        s.insert_char('!');
        assert_eq!(s.fields[0].value.len(), len + 1);
        s.backspace();
        assert_eq!(s.fields[0].value.len(), len);
    }

    #[test]
    fn number_field_rejects_non_digits() {
        let mut s = make_settings();
        // "Refresh interval (secs)" is now index 4
        s.selected = 4;
        s.start_edit();
        let before = s.fields[4].value.clone();
        s.insert_char('a');
        assert_eq!(s.fields[4].value, before); // unchanged
        s.insert_char('5');
        assert!(s.fields[4].value.ends_with('5'));
    }

    #[test]
    fn to_config_round_trip() {
        let original = make_config();
        let s = SettingsState::from_config(&original, PathBuf::from("/tmp/test.toml"));
        let rebuilt = s.to_config();

        assert_eq!(rebuilt.azure_devops.organization, "testorg");
        assert_eq!(rebuilt.azure_devops.project, "testproj");
        assert!(rebuilt.filters.folders.is_empty());
        assert!(rebuilt.filters.definition_ids.is_empty());
        assert!(rebuilt.update.check_for_updates);
        assert_eq!(rebuilt.logging.level, "info");
        assert!(rebuilt.notifications.enabled);
        assert_eq!(rebuilt.display.refresh_interval_secs, 15);
        assert_eq!(rebuilt.display.log_refresh_interval_secs, 5);
    }

    #[test]
    fn to_config_enforces_minimums() {
        let mut config = make_config();
        config.display.refresh_interval_secs = 1; // below min of 5
        config.display.log_refresh_interval_secs = 0; // below min of 1
        let s = SettingsState::from_config(&config, PathBuf::from("/tmp/test.toml"));
        let rebuilt = s.to_config();
        assert_eq!(rebuilt.display.refresh_interval_secs, 5);
        assert_eq!(rebuilt.display.log_refresh_interval_secs, 1);
    }
}
