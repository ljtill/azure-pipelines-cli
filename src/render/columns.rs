//! Canonical column schemas shared across list views.

use crate::render::table::{Align, Column, ColumnWidth};

/// Options controlling which optional columns appear in a build-row schema.
#[derive(Debug, Clone, Copy, Default)]
pub struct BuildRowOpts {
    pub select: bool,
    pub name: bool,
    pub retained: bool,
}

/// A resolved build-row schema: the ordered column list plus named indices so
/// view code can reference cells by purpose, not position.
#[derive(Debug, Clone)]
pub struct BuildRowSchema {
    pub columns: Vec<Column>,
    pub select: Option<usize>,
    pub icon: usize,
    pub status: usize,
    pub name: Option<usize>,
    pub build_number: usize,
    pub retained: Option<usize>,
    pub branch: usize,
    pub requestor: usize,
    pub elapsed: usize,
}

/// Builds the canonical build-row column schema with optional columns toggled.
///
/// Ordering (left to right):
/// `[select?] icon status [name?] build# [retained?] branch requestor elapsed`.
#[must_use]
pub fn build_row(opts: BuildRowOpts) -> BuildRowSchema {
    let mut columns: Vec<Column> = Vec::with_capacity(9);
    let mut select_idx = None;
    let mut name_idx = None;
    let mut retained_idx = None;

    if opts.select {
        select_idx = Some(columns.len());
        columns.push(Column {
            label: "",
            width: ColumnWidth::Fixed(2),
            align: Align::Left,
        });
    }

    let icon = columns.len();
    columns.push(Column {
        label: "",
        width: ColumnWidth::Fixed(4),
        align: Align::Left,
    });

    let status = columns.len();
    columns.push(Column {
        label: "Status",
        width: ColumnWidth::Fixed(12),
        align: Align::Left,
    });

    if opts.name {
        name_idx = Some(columns.len());
        columns.push(Column {
            label: "Pipeline",
            width: ColumnWidth::Flex {
                weight: 3,
                min: 20,
                max: None,
            },
            align: Align::Left,
        });
    }

    let build_number = columns.len();
    columns.push(Column {
        label: "Build",
        width: ColumnWidth::Fixed(14),
        align: Align::Left,
    });

    if opts.retained {
        retained_idx = Some(columns.len());
        columns.push(Column {
            label: "",
            width: ColumnWidth::Fixed(2),
            align: Align::Left,
        });
    }

    let branch = columns.len();
    columns.push(Column {
        label: "Branch",
        width: ColumnWidth::Flex {
            weight: 2,
            min: 12,
            max: Some(30),
        },
        align: Align::Left,
    });

    let requestor = columns.len();
    columns.push(Column {
        label: "Requestor",
        width: ColumnWidth::Flex {
            weight: 2,
            min: 12,
            max: Some(25),
        },
        align: Align::Left,
    });

    let elapsed = columns.len();
    columns.push(Column {
        label: "Elapsed",
        width: ColumnWidth::Fixed(12),
        align: Align::Right,
    });

    BuildRowSchema {
        columns,
        select: select_idx,
        icon,
        status,
        name: name_idx,
        build_number,
        retained: retained_idx,
        branch,
        requestor,
        elapsed,
    }
}

/// A resolved pull-request row schema.
#[derive(Debug, Clone)]
pub struct PullRequestSchema {
    pub columns: Vec<Column>,
    pub icon: usize,
    pub title: usize,
    pub repo: usize,
    pub branch: usize,
    pub votes: usize,
}

/// Builds the canonical pull-request row column schema.
///
/// Ordering (left to right):
/// `icon title repo branch votes`.
#[must_use]
pub fn pull_request_row() -> PullRequestSchema {
    let columns = vec![
        Column {
            label: "",
            width: ColumnWidth::Fixed(4),
            align: Align::Left,
        },
        Column {
            label: "Title",
            width: ColumnWidth::Flex {
                weight: 3,
                min: 30,
                max: None,
            },
            align: Align::Left,
        },
        Column {
            label: "Repo",
            width: ColumnWidth::Flex {
                weight: 1,
                min: 12,
                max: Some(24),
            },
            align: Align::Left,
        },
        Column {
            label: "Target",
            width: ColumnWidth::Flex {
                weight: 2,
                min: 14,
                max: Some(28),
            },
            align: Align::Left,
        },
        Column {
            label: "Votes",
            width: ColumnWidth::Fixed(14),
            align: Align::Left,
        },
    ];
    PullRequestSchema {
        columns,
        icon: 0,
        title: 1,
        repo: 2,
        branch: 3,
        votes: 4,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_row_full_has_all_columns() {
        let s = build_row(BuildRowOpts {
            select: true,
            name: true,
            retained: true,
        });
        assert_eq!(s.columns.len(), 9);
        assert_eq!(s.select, Some(0));
        assert_eq!(s.icon, 1);
        assert_eq!(s.status, 2);
        assert_eq!(s.name, Some(3));
        assert_eq!(s.build_number, 4);
        assert_eq!(s.retained, Some(5));
        assert_eq!(s.branch, 6);
        assert_eq!(s.requestor, 7);
        assert_eq!(s.elapsed, 8);
    }

    #[test]
    fn build_row_minimal_omits_optional_columns() {
        let s = build_row(BuildRowOpts::default());
        assert_eq!(s.columns.len(), 6);
        assert!(s.select.is_none());
        assert!(s.name.is_none());
        assert!(s.retained.is_none());
        assert_eq!(s.icon, 0);
        assert_eq!(s.elapsed, 5);
    }

    #[test]
    fn build_row_history_flavour() {
        // Build History: select + retained but no name column.
        let s = build_row(BuildRowOpts {
            select: true,
            name: false,
            retained: true,
        });
        assert_eq!(s.columns.len(), 8);
        assert_eq!(s.select, Some(0));
        assert!(s.name.is_none());
        assert_eq!(s.retained, Some(4));
    }

    #[test]
    fn pull_request_row_has_expected_columns() {
        let s = pull_request_row();
        assert_eq!(s.columns.len(), 5);
        assert_eq!(s.icon, 0);
        assert_eq!(s.title, 1);
        assert_eq!(s.repo, 2);
        assert_eq!(s.branch, 3);
        assert_eq!(s.votes, 4);
    }
}
