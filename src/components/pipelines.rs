//! Pipelines view component showing all pipeline definitions grouped by folder.

use std::collections::{BTreeMap, HashSet};

use anyhow::Result;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState};

use super::Component;
use crate::client::models::{Build, PipelineDefinition};
use crate::render::columns::{BuildRowOpts, build_row};
use crate::render::helpers::{
    build_elapsed, draw_state_message, draw_view_frame, effective_status_icon,
    effective_status_label, row_style, split_with_search_bar, truncate,
};
use crate::render::table::{render_header, resolve_widths};
use crate::render::theme;
use crate::state::nav::ListNav;
use crate::state::{App, InputMode};

/// Represents a row in the folder-grouped pipeline view.
#[derive(Debug, Clone)]
pub enum PipelineRow {
    FolderHeader {
        key: String,
        label: String,
        depth: usize,
        expanded: bool,
    },
    Pipeline {
        definition: PipelineDefinition,
        latest_build: Option<Box<Build>>,
        pinned: bool,
        depth: usize,
    },
}

/// Normalizes an ADO definition path to a canonical folder key.
///
/// Azure DevOps pipeline definition paths are conventionally of the form
/// `\Folder\Subfolder` with a leading backslash, or `\` for the root. We
/// rely on that convention for grouping and display, but tolerate missing
/// or malformed inputs defensively:
///
/// * empty string -> root (`\`)
/// * `\` -> root
/// * anything else without a leading backslash is prefixed with `\` so it
///   still groups consistently with well-formed paths.
fn folder_key(path: &str) -> String {
    if path.is_empty() || path == "\\" {
        "\\".to_string()
    } else if path.starts_with('\\') {
        path.to_string()
    } else {
        // Defensive: ADO should always prefix paths with `\`, but if a response
        // ever omits it, normalize so grouping/collapsing behavior is stable.
        format!("\\{path}")
    }
}

/// Returns the leaf segment of a folder key (e.g. `\A\B\C` -> `C`).
fn folder_leaf_label(key: &str) -> String {
    key.rsplit('\\')
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Returns the list of ancestor folder keys for a given key, root-first.
/// For `\A\B\C` this yields [`\A`, `\A\B`]; the input key itself is not included.
fn folder_ancestors(key: &str) -> Vec<String> {
    let segments: Vec<&str> = key.split('\\').filter(|s| !s.is_empty()).collect();
    let mut out = Vec::with_capacity(segments.len().saturating_sub(1));
    let mut current = String::new();
    for seg in segments.iter().take(segments.len().saturating_sub(1)) {
        current.push('\\');
        current.push_str(seg);
        out.push(current.clone());
    }
    out
}

/// Checks if a definition passes the configured filters.
fn matches_filter(
    def: &PipelineDefinition,
    filter_definition_ids: &[u32],
    filter_folders: &[String],
) -> bool {
    if !filter_definition_ids.is_empty() && !filter_definition_ids.contains(&def.id) {
        return false;
    }
    if !filter_folders.is_empty() && !filter_folders.iter().any(|f| def.path.starts_with(f)) {
        return false;
    }
    true
}

/// Tree node for hierarchical folder grouping.
#[derive(Debug, Default)]
struct FolderNode {
    /// Child folder full keys, keyed by leaf segment for deterministic ordering.
    children: BTreeMap<String, String>,
    /// Pipelines directly contained in this folder.
    pipelines: Vec<(PipelineDefinition, Option<Build>)>,
}

/// Renders pipelines grouped by folder with collapse/expand, search, and pinning.
#[derive(Debug, Default)]
pub struct Pipelines {
    pub rows: Vec<PipelineRow>,
    /// Folder keys that the user has explicitly expanded. Default state is collapsed.
    pub expanded_folders: HashSet<String>,
    pub nav: ListNav,
    pub selected: HashSet<u32>,
}

impl Pipelines {
    /// Rebuilds the pipeline rows from definitions + latest builds, grouped by folder.
    pub fn rebuild(
        &mut self,
        definitions: &[PipelineDefinition],
        latest_builds_by_def: &BTreeMap<u32, Build>,
        filter_folders: &[String],
        filter_definition_ids: &[u32],
        pinned_definition_ids: &[u32],
        search_query: &str,
    ) {
        let query_lower = search_query.to_lowercase();
        let has_query = !search_query.is_empty();

        let mut tree: BTreeMap<String, FolderNode> = BTreeMap::new();
        tree.entry("\\".to_string()).or_default();
        let mut auto_expanded: HashSet<String> = HashSet::new();

        for def in definitions {
            if !matches_filter(def, filter_definition_ids, filter_folders) {
                continue;
            }
            if has_query
                && !def.name.to_lowercase().contains(&query_lower)
                && !def.path.to_lowercase().contains(&query_lower)
            {
                continue;
            }

            let key = folder_key(&def.path);
            // Register every ancestor so intermediate folders show up as headers.
            let mut parent = "\\".to_string();
            let segments: Vec<&str> = key.split('\\').filter(|s| !s.is_empty()).collect();
            for seg in &segments {
                let child_key = if parent == "\\" {
                    format!("\\{seg}")
                } else {
                    format!("{parent}\\{seg}")
                };
                tree.entry(parent.clone())
                    .or_default()
                    .children
                    .insert((*seg).to_string(), child_key.clone());
                tree.entry(child_key.clone()).or_default();
                parent = child_key;
            }

            let latest = latest_builds_by_def.get(&def.id).cloned();
            tree.entry(key.clone())
                .or_default()
                .pipelines
                .push((def.clone(), latest));

            // When a query is active, auto-expand ancestors of every match so
            // results are visible without requiring manual drill-in.
            if has_query {
                auto_expanded.insert(key.clone());
                for anc in folder_ancestors(&key) {
                    auto_expanded.insert(anc);
                }
            }
        }

        let mut rows = Vec::new();
        self.emit_root(&tree, &auto_expanded, pinned_definition_ids, &mut rows);

        self.rows = rows;
        self.nav.set_len(self.rows.len());
    }

    /// Emits rows for the root folder's contents. Root has no header — its
    /// children render at depth 0 directly, alongside any root-level pipelines.
    fn emit_root(
        &self,
        tree: &BTreeMap<String, FolderNode>,
        auto_expanded: &HashSet<String>,
        pinned_ids: &[u32],
        rows: &mut Vec<PipelineRow>,
    ) {
        let Some(root) = tree.get("\\") else {
            return;
        };
        for child_key in root.children.values() {
            self.emit_folder(child_key, 0, tree, auto_expanded, pinned_ids, rows);
        }
        let mut root_pipelines = root.pipelines.clone();
        root_pipelines.sort_by_key(|(a, _)| a.name.to_lowercase());
        for (def, build) in root_pipelines {
            let pinned = pinned_ids.contains(&def.id);
            rows.push(PipelineRow::Pipeline {
                definition: def,
                latest_build: build.map(Box::new),
                pinned,
                depth: 0,
            });
        }
    }

    /// Emits a folder header and, if expanded, its children (subfolders first, then pipelines).
    fn emit_folder(
        &self,
        key: &str,
        depth: usize,
        tree: &BTreeMap<String, FolderNode>,
        auto_expanded: &HashSet<String>,
        pinned_ids: &[u32],
        rows: &mut Vec<PipelineRow>,
    ) {
        let Some(node) = tree.get(key) else {
            return;
        };
        let expanded = self.expanded_folders.contains(key) || auto_expanded.contains(key);
        rows.push(PipelineRow::FolderHeader {
            key: key.to_string(),
            label: folder_leaf_label(key),
            depth,
            expanded,
        });
        if !expanded {
            return;
        }
        for child_key in node.children.values() {
            self.emit_folder(child_key, depth + 1, tree, auto_expanded, pinned_ids, rows);
        }
        let mut pipelines = node.pipelines.clone();
        pipelines.sort_by_key(|(a, _)| a.name.to_lowercase());
        for (def, build) in pipelines {
            let pinned = pinned_ids.contains(&def.id);
            rows.push(PipelineRow::Pipeline {
                definition: def,
                latest_build: build.map(Box::new),
                pinned,
                depth: depth + 1,
            });
        }
    }

    /// Toggles collapse state for a folder at the given row index.
    pub fn toggle_folder_at(&mut self, index: usize) -> bool {
        if let Some(PipelineRow::FolderHeader { key, .. }) = self.rows.get(index) {
            let key = key.clone();
            if self.expanded_folders.contains(&key) {
                self.expanded_folders.remove(&key);
            } else {
                self.expanded_folders.insert(key);
            }
            return true;
        }
        false
    }

    /// Collapses the folder at the given index.
    pub fn collapse_folder_at(&mut self, index: usize) -> bool {
        if let Some(PipelineRow::FolderHeader { key, expanded, .. }) = self.rows.get(index)
            && *expanded
        {
            let key = key.clone();
            self.expanded_folders.remove(&key);
            return true;
        }
        false
    }

    /// Expands the folder at the given index.
    pub fn expand_folder_at(&mut self, index: usize) -> bool {
        if let Some(PipelineRow::FolderHeader { key, expanded, .. }) = self.rows.get(index)
            && !*expanded
        {
            let key = key.clone();
            self.expanded_folders.insert(key);
            return true;
        }
        false
    }

    /// Finds the row index of the immediate parent folder header for a row.
    /// Returns `None` for depth-0 rows (no parent header in the hierarchy).
    pub fn find_parent_folder_index(&self, row_index: usize) -> Option<usize> {
        let current_depth = match self.rows.get(row_index)? {
            PipelineRow::FolderHeader { depth, .. } | PipelineRow::Pipeline { depth, .. } => *depth,
        };
        if current_depth == 0 {
            return None;
        }
        let target_depth = current_depth - 1;
        for i in (0..row_index).rev() {
            if let Some(PipelineRow::FolderHeader { depth, .. }) = self.rows.get(i)
                && *depth == target_depth
            {
                return Some(i);
            }
        }
        None
    }

    /// Checks if a row is a folder header.
    pub fn is_folder_header(&self, index: usize) -> bool {
        matches!(self.rows.get(index), Some(PipelineRow::FolderHeader { .. }))
    }

    /// Returns the definition at the given row index, if it is a pipeline row.
    pub fn definition_at(&self, index: usize) -> Option<&PipelineDefinition> {
        match self.rows.get(index) {
            Some(PipelineRow::Pipeline { definition, .. }) => Some(definition),
            _ => None,
        }
    }

    /// Renders the folder-grouped pipeline list.
    pub fn draw_with_app(&self, f: &mut Frame, app: &App, area: Rect) {
        let show_search = app.search.mode == InputMode::Search || !app.search.query.is_empty();
        let selected_count = self.selected.len();
        let mut subtitle_spans = crate::render::helpers::sub_view_tab_spans(app.service, app.view);
        if !subtitle_spans.is_empty() {
            subtitle_spans.push(Span::styled("  ·  ", theme::MUTED));
        }
        subtitle_spans.push(Span::styled(
            format!("{} pipelines", self.pipeline_count()),
            theme::TEXT,
        ));
        subtitle_spans.push(Span::styled(
            format!("  ·  {selected_count} selected"),
            if selected_count > 0 {
                theme::SUCCESS
            } else {
                theme::MUTED
            },
        ));
        let subtitle = Line::from(subtitle_spans);
        let frame_area = draw_view_frame(f, area, " Pipelines ", Some(subtitle));
        let list_area = split_with_search_bar(
            f,
            frame_area,
            &app.search.query,
            app.search.mode,
            show_search,
        );

        if self.rows.is_empty() {
            let hint = if show_search {
                " No pipelines match the current search"
            } else {
                " No pipelines found"
            };
            draw_state_message(f, list_area, hint, theme::MUTED);
            return;
        }

        let schema = build_row(BuildRowOpts {
            select: true,
            name: true,
            retained: false,
        });
        let list_area = render_header(f, list_area, &schema.columns);
        let resolved = resolve_widths(&schema.columns, list_area.width);
        let widths: Vec<usize> = resolved.iter().map(|&w| w as usize).collect();

        let items: Vec<ListItem> = self
            .rows
            .iter()
            .enumerate()
            .map(|(i, row)| match row {
                PipelineRow::FolderHeader {
                    label,
                    depth,
                    expanded,
                    ..
                } => {
                    let icon = if *expanded { "▾" } else { "▸" };
                    let indent = "  ".repeat(*depth);
                    ListItem::new(Line::from(vec![
                        Span::raw(indent),
                        Span::styled(format!(" {icon} "), theme::ARROW),
                        Span::styled(label.clone(), theme::FOLDER),
                    ]))
                    .style(row_style(i == self.nav.index()))
                }
                PipelineRow::Pipeline {
                    definition,
                    latest_build,
                    pinned,
                    depth,
                } => {
                    let (icon, icon_color) =
                        latest_build.as_ref().map_or(("○", Color::DarkGray), |b| {
                            let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                            effective_status_icon(b.status, b.result, awaiting)
                        });
                    let label = latest_build.as_ref().map_or("", |b| {
                        let awaiting = app.data.pending_approval_build_ids.contains(&b.id);
                        effective_status_label(b.status, b.result, awaiting)
                    });

                    let build_spans = latest_build.as_ref().map_or_else(
                        || vec![Span::styled("no builds", theme::MUTED)],
                        |b| {
                            let branch = b.branch_display();
                            let elapsed = build_elapsed(b);
                            vec![
                                Span::styled(
                                    format!(
                                        "#{:<width$}",
                                        truncate(&b.build_number, widths[4] - 1),
                                        width = widths[4] - 1
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!(
                                        "{:<width$} ",
                                        truncate(&branch, widths[5].saturating_sub(1)),
                                        width = widths[5].saturating_sub(1)
                                    ),
                                    theme::BRANCH,
                                ),
                                Span::styled(
                                    format!(
                                        "{:<width$} ",
                                        truncate(b.requestor(), widths[6].saturating_sub(1)),
                                        width = widths[6].saturating_sub(1)
                                    ),
                                    theme::MUTED,
                                ),
                                Span::styled(
                                    format!("{:>width$}", elapsed, width = widths[7]),
                                    theme::MUTED,
                                ),
                            ]
                        },
                    );

                    let pin_indicator = if *pinned { "★ " } else { "" };
                    let selected_indicator = if self.selected.contains(&definition.id) {
                        "✓ "
                    } else {
                        "  "
                    };
                    let name_style = if latest_build.is_some() {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };
                    let indent = "  ".repeat(*depth);

                    let mut spans = vec![
                        Span::raw(indent),
                        Span::styled(selected_indicator, theme::SUCCESS),
                        Span::styled(format!("{icon} "), Style::new().fg(icon_color)),
                        Span::styled(
                            format!("{:<width$}", label, width = widths[2]),
                            Style::new().fg(icon_color),
                        ),
                        Span::styled(
                            format!(
                                "{}{:<width$} ",
                                pin_indicator,
                                truncate(
                                    &definition.name,
                                    widths[3]
                                        .saturating_sub(1)
                                        .saturating_sub(pin_indicator.len()),
                                ),
                                width = widths[3]
                                    .saturating_sub(1)
                                    .saturating_sub(pin_indicator.len())
                            ),
                            name_style,
                        ),
                    ];

                    spans.extend(build_spans);

                    ListItem::new(Line::from(spans)).style(row_style(i == self.nav.index()))
                }
            })
            .collect();
        let list = List::new(items).scroll_padding(3);

        let mut state = ListState::default();
        state.select(Some(self.nav.index()));
        f.render_stateful_widget(list, list_area, &mut state);
    }

    /// Returns the number of pipeline rows (excluding folder headers).
    fn pipeline_count(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r, PipelineRow::Pipeline { .. }))
            .count()
    }
}

impl Component for Pipelines {
    fn draw(&self, _frame: &mut Frame, _area: Rect) -> Result<()> {
        Ok(())
    }

    fn footer_hints(&self) -> &'static str {
        "↑↓ navigate  ←→ collapse/expand  Enter drill-in  Space select  p pin  Q queue  o open  / search  1–4 areas  ? help"
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::test_helpers::*;

    #[test]
    fn folder_key_root() {
        assert_eq!(folder_key(""), "\\");
        assert_eq!(folder_key("\\"), "\\");
    }

    #[test]
    fn folder_key_nested() {
        assert_eq!(folder_key("\\Infra"), "\\Infra");
    }

    #[test]
    fn folder_leaf_label_works() {
        assert_eq!(folder_leaf_label("\\"), "");
        assert_eq!(folder_leaf_label("\\Infra"), "Infra");
        assert_eq!(folder_leaf_label("\\A\\B\\C"), "C");
    }

    #[test]
    fn folder_ancestors_works() {
        assert!(folder_ancestors("\\").is_empty());
        assert!(folder_ancestors("\\A").is_empty());
        assert_eq!(folder_ancestors("\\A\\B"), vec!["\\A".to_string()]);
        assert_eq!(
            folder_ancestors("\\A\\B\\C"),
            vec!["\\A".to_string(), "\\A\\B".to_string()]
        );
    }

    #[test]
    fn folder_key_handles_malformed_paths() {
        // Empty path collapses to root.
        assert_eq!(folder_key(""), "\\");
        // Single backslash is root.
        assert_eq!(folder_key("\\"), "\\");
        // Well-formed nested paths pass through.
        assert_eq!(folder_key("\\Infra"), "\\Infra");
        assert_eq!(folder_key("\\Infra\\Deploy"), "\\Infra\\Deploy");
        // Missing leading backslash is normalized to be safe.
        assert_eq!(folder_key("Infra"), "\\Infra");
        assert_eq!(folder_key("Infra\\Deploy"), "\\Infra\\Deploy");
    }

    #[test]
    fn rebuild_default_collapsed_shows_root_headers_only() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
            make_definition(3, "Lint", "\\"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        // Expect: Infra folder header (collapsed), then two root pipelines.
        assert_eq!(p.rows.len(), 3);
        assert!(matches!(
            &p.rows[0],
            PipelineRow::FolderHeader { label, depth: 0, expanded: false, .. } if label == "Infra"
        ));
        assert!(matches!(&p.rows[1], PipelineRow::Pipeline { depth: 0, .. }));
        assert!(matches!(&p.rows[2], PipelineRow::Pipeline { depth: 0, .. }));
    }

    #[test]
    fn rebuild_builds_nested_tree_with_depths() {
        let defs = vec![make_definition(1, "Leaf", "\\A\\B\\C")];
        let mut p = Pipelines::default();
        // Expand the full chain so all three headers and the pipeline appear.
        p.expanded_folders.insert("\\A".to_string());
        p.expanded_folders.insert("\\A\\B".to_string());
        p.expanded_folders.insert("\\A\\B\\C".to_string());
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 4);
        assert!(matches!(
            &p.rows[0],
            PipelineRow::FolderHeader { depth: 0, label, .. } if label == "A"
        ));
        assert!(matches!(
            &p.rows[1],
            PipelineRow::FolderHeader { depth: 1, label, .. } if label == "B"
        ));
        assert!(matches!(
            &p.rows[2],
            PipelineRow::FolderHeader { depth: 2, label, .. } if label == "C"
        ));
        assert!(matches!(&p.rows[3], PipelineRow::Pipeline { depth: 3, .. }));
    }

    #[test]
    fn expanding_root_folder_reveals_direct_children_only() {
        let defs = vec![make_definition(1, "Leaf", "\\A\\B\\C")];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 1); // Just the `A` header.
        // Expand `\A` only.
        p.expanded_folders.insert("\\A".to_string());
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        // Should show `A` (expanded) + `B` (still collapsed), but not `C` or the pipeline.
        assert_eq!(p.rows.len(), 2);
        assert!(matches!(
            &p.rows[1],
            PipelineRow::FolderHeader { depth: 1, label, expanded: false, .. } if label == "B"
        ));
    }

    #[test]
    fn rebuild_search_filters_pipelines() {
        let defs = vec![
            make_definition(1, "CI Pipeline", "\\"),
            make_definition(2, "Deploy", "\\Infra"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "ci");
        // Single root-level pipeline matches; no header for root.
        assert_eq!(p.rows.len(), 1);
        assert!(matches!(&p.rows[0], PipelineRow::Pipeline { depth: 0, .. }));
    }

    #[test]
    fn search_auto_expands_ancestor_chain() {
        let defs = vec![make_definition(1, "Leaf", "\\A\\B\\C")];
        let mut p = Pipelines::default();
        // No explicit expansion — search should auto-expand ancestors.
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "Leaf");
        assert_eq!(p.rows.len(), 4);
    }

    #[test]
    fn rebuild_marks_pinned() {
        let defs = vec![
            make_definition(1, "CI", "\\"),
            make_definition(2, "Deploy", "\\"),
        ];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[1], "");
        let pinned: Vec<bool> = p
            .rows
            .iter()
            .filter_map(|r| match r {
                PipelineRow::Pipeline { pinned, .. } => Some(*pinned),
                PipelineRow::FolderHeader { .. } => None,
            })
            .collect();
        assert_eq!(pinned, vec![true, false]);
    }

    #[test]
    fn toggle_folder_collapses_and_expands() {
        let defs = vec![make_definition(1, "Deploy", "\\Infra")];
        let mut p = Pipelines::default();
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        // Default collapsed: only `Infra` header visible.
        assert_eq!(p.rows.len(), 1);

        // Toggle → expand.
        p.toggle_folder_at(0);
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 2);

        // Toggle → collapse again.
        p.toggle_folder_at(0);
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        assert_eq!(p.rows.len(), 1);
    }

    #[test]
    fn find_parent_folder_index_walks_to_immediate_parent() {
        let defs = vec![make_definition(1, "Leaf", "\\A\\B")];
        let mut p = Pipelines::default();
        p.expanded_folders.insert("\\A".to_string());
        p.expanded_folders.insert("\\A\\B".to_string());
        p.rebuild(&defs, &BTreeMap::new(), &[], &[], &[], "");
        // rows: [A (d0), B (d1), Leaf (d2)].
        assert_eq!(p.find_parent_folder_index(2), Some(1));
        assert_eq!(p.find_parent_folder_index(1), Some(0));
        assert_eq!(p.find_parent_folder_index(0), None);
    }

    #[test]
    fn matches_filter_no_filters() {
        assert!(matches_filter(&make_definition(1, "P", "\\"), &[], &[]));
    }

    #[test]
    fn matches_filter_by_id() {
        assert!(matches_filter(&make_definition(1, "P", "\\"), &[1, 2], &[]));
        assert!(!matches_filter(
            &make_definition(99, "P", "\\"),
            &[1, 2],
            &[]
        ));
    }

    #[test]
    fn matches_filter_by_folder() {
        let folders = vec!["\\Infra".to_string()];
        assert!(matches_filter(
            &make_definition(1, "P", "\\Infra"),
            &[],
            &folders
        ));
        assert!(!matches_filter(
            &make_definition(2, "P", "\\"),
            &[],
            &folders
        ));
    }
}
