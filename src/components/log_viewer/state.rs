use std::collections::HashSet;

use ratatui::layout::Rect;

use crate::api::models::{Build, BuildTimeline};

use crate::app::View;
use crate::app::nav::ListNav;

use super::TimelineRow;

/// State for the log viewer screen — reset as a unit on navigation.
pub struct LogViewer {
    pub selected_build: Option<Build>,
    pub build_timeline: Option<BuildTimeline>,
    pub timeline_rows: Vec<TimelineRow>,
    pub collapsed_stages: HashSet<String>,
    pub collapsed_jobs: HashSet<String>,
    pub log_content: Vec<String>,
    pub log_auto_scroll: bool,
    pub log_generation: u64,
    pub timeline_initialized: bool,
    pub follow_mode: bool,
    pub followed_task_name: String,
    pub followed_log_id: Option<u32>,
    pub log_entries_nav: ListNav,
    pub log_scroll_offset: u32,
    /// Cached layout areas from the last render, used for mouse hit-testing.
    pub tree_area: Option<Rect>,
    pub log_area: Option<Rect>,
    /// The view to return to when pressing Esc from LogViewer.
    pub return_to_view: View,
}

impl Default for LogViewer {
    fn default() -> Self {
        Self {
            selected_build: None,
            build_timeline: None,
            timeline_rows: Vec::new(),
            collapsed_stages: HashSet::new(),
            collapsed_jobs: HashSet::new(),
            log_content: Vec::new(),
            log_auto_scroll: false,
            log_generation: 0,
            timeline_initialized: false,
            follow_mode: false,
            followed_task_name: String::new(),
            followed_log_id: None,
            log_entries_nav: ListNav::default(),
            log_scroll_offset: 0,
            tree_area: None,
            log_area: None,
            return_to_view: View::BuildHistory,
        }
    }
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------
impl LogViewer {
    /// Create a new log viewer state for navigating to a specific build.
    pub fn new_for_build(build: Build, return_to: View, generation: u64) -> Self {
        Self {
            selected_build: Some(build),
            log_auto_scroll: true,
            follow_mode: true,
            log_generation: generation,
            return_to_view: return_to,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Getters
// ---------------------------------------------------------------------------
impl LogViewer {
    pub fn selected_build(&self) -> Option<&Build> {
        self.selected_build.as_ref()
    }

    #[allow(dead_code)]
    pub fn build_timeline(&self) -> Option<&BuildTimeline> {
        self.build_timeline.as_ref()
    }

    pub fn timeline_rows(&self) -> &[TimelineRow] {
        &self.timeline_rows
    }

    pub fn log_content(&self) -> &[String] {
        &self.log_content
    }

    pub fn log_auto_scroll(&self) -> bool {
        self.log_auto_scroll
    }

    pub fn generation(&self) -> u64 {
        self.log_generation
    }

    pub fn is_following(&self) -> bool {
        self.follow_mode
    }

    pub fn followed_task_name(&self) -> &str {
        &self.followed_task_name
    }

    pub fn followed_log_id(&self) -> Option<u32> {
        self.followed_log_id
    }

    pub fn log_scroll_offset(&self) -> u32 {
        self.log_scroll_offset
    }

    pub fn return_to_view(&self) -> View {
        self.return_to_view
    }

    pub fn nav(&self) -> &ListNav {
        &self.log_entries_nav
    }

    pub fn nav_mut(&mut self) -> &mut ListNav {
        &mut self.log_entries_nav
    }

    pub fn tree_area(&self) -> Option<Rect> {
        self.tree_area
    }

    pub fn log_area(&self) -> Option<Rect> {
        self.log_area
    }
}

// ---------------------------------------------------------------------------
// Mutators
// ---------------------------------------------------------------------------
impl LogViewer {
    pub fn set_build_timeline(&mut self, timeline: BuildTimeline) {
        self.build_timeline = Some(timeline);
    }

    pub fn set_log_content(&mut self, content: String) {
        self.log_content = content.lines().map(String::from).collect();
        self.log_auto_scroll = true;
        self.log_scroll_offset = 0;
    }

    pub fn clear_log(&mut self) {
        self.log_content.clear();
    }

    #[allow(dead_code)]
    pub fn set_log_auto_scroll(&mut self, auto: bool) {
        self.log_auto_scroll = auto;
    }

    #[allow(dead_code)]
    pub fn set_log_scroll_offset(&mut self, offset: u32) {
        self.log_scroll_offset = offset;
    }

    pub fn set_generation(&mut self, generation: u64) {
        self.log_generation = generation;
    }

    pub fn scroll_up(&mut self, amount: u32) {
        self.log_auto_scroll = false;
        self.log_scroll_offset = self.log_scroll_offset.saturating_sub(amount);
    }

    pub fn scroll_down(&mut self, amount: u32) {
        self.log_scroll_offset = self.log_scroll_offset.saturating_add(amount);
    }

    pub fn set_layout_areas(&mut self, tree: Rect, log: Rect) {
        self.tree_area = Some(tree);
        self.log_area = Some(log);
    }

    #[allow(dead_code)]
    pub fn set_timeline_rows(&mut self, rows: Vec<TimelineRow>) {
        self.timeline_rows = rows;
        self.log_entries_nav.set_len(self.timeline_rows.len());
    }
}

// ---------------------------------------------------------------------------
// Timeline collapse state
// ---------------------------------------------------------------------------
impl LogViewer {
    #[allow(dead_code)]
    pub fn is_stage_collapsed(&self, id: &str) -> bool {
        self.collapsed_stages.contains(id)
    }

    #[allow(dead_code)]
    pub fn is_job_collapsed(&self, id: &str) -> bool {
        self.collapsed_jobs.contains(id)
    }

    pub fn collapse_stage(&mut self, id: String) {
        self.collapsed_stages.insert(id);
    }

    pub fn expand_stage(&mut self, id: &str) {
        self.collapsed_stages.remove(id);
    }

    pub fn collapse_job(&mut self, id: String) {
        self.collapsed_jobs.insert(id);
    }

    pub fn expand_job(&mut self, id: &str) {
        self.collapsed_jobs.remove(id);
    }

    pub fn toggle_stage(&mut self, id: &str) -> bool {
        if self.collapsed_stages.contains(id) {
            self.collapsed_stages.remove(id);
            false
        } else {
            self.collapsed_stages.insert(id.to_owned());
            true
        }
    }

    pub fn toggle_job(&mut self, id: &str) -> bool {
        if self.collapsed_jobs.contains(id) {
            self.collapsed_jobs.remove(id);
            false
        } else {
            self.collapsed_jobs.insert(id.to_owned());
            true
        }
    }

    #[allow(dead_code)]
    pub fn is_timeline_initialized(&self) -> bool {
        self.timeline_initialized
    }

    #[allow(dead_code)]
    pub fn set_timeline_initialized(&mut self) {
        self.timeline_initialized = true;
    }
}
