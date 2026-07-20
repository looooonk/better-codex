use super::diff_path::DiffPath;
use super::diff_path::bounded_visible_path;
use super::diff_path::header_path;
use super::diff_path::metadata_path;
use super::diff_path::parse_git_paths;
use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;
use std::path::Path;

pub(super) use metadata::DiffMetadata;

mod metadata;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffFileKind {
    Added,
    Deleted,
    Modified,
    Renamed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffStatus {
    InProgress,
    Completed,
    Failed,
    Declined,
}

impl DiffStatus {
    pub(super) fn is_session_edit(self) -> bool {
        matches!(self, Self::InProgress | Self::Completed)
    }
}

impl From<PatchApplyStatus> for DiffStatus {
    fn from(status: PatchApplyStatus) -> Self {
        match status {
            PatchApplyStatus::InProgress => Self::InProgress,
            PatchApplyStatus::Completed => Self::Completed,
            PatchApplyStatus::Failed => Self::Failed,
            PatchApplyStatus::Declined => Self::Declined,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffLineKind {
    Context,
    Added,
    Removed,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffCell {
    pub(super) line_number: Option<usize>,
    pub(super) text: String,
    pub(super) kind: DiffLineKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffRow {
    pub(super) old: Option<DiffCell>,
    pub(super) new: Option<DiffCell>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct DiffStats {
    pub(super) files: usize,
    pub(super) additions: usize,
    pub(super) removals: usize,
}

impl DiffStats {
    pub(super) fn from_files<'a>(files: impl IntoIterator<Item = &'a DiffFile>) -> Self {
        files.into_iter().fold(Self::default(), |mut total, file| {
            let stats = file.stats();
            total.files += stats.files;
            total.additions += stats.additions;
            total.removals += stats.removals;
            total
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffFile {
    old_path: Option<DiffPath>,
    new_path: Option<DiffPath>,
    kind: DiffFileKind,
    status: DiffStatus,
    rows: Vec<DiffRow>,
    metadata: DiffMetadata,
}

impl DiffFile {
    pub(super) fn added(
        path: impl Into<String>,
        content: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        let path = path.into();
        Self {
            old_path: None,
            new_path: Some(DiffPath::new(path)),
            kind: DiffFileKind::Added,
            status,
            rows: content
                .as_ref()
                .lines()
                .enumerate()
                .map(|(index, text)| DiffRow {
                    old: None,
                    new: Some(cell(Some(index + 1), text, DiffLineKind::Added)),
                })
                .collect(),
            metadata: DiffMetadata::default(),
        }
    }

    pub(super) fn deleted(
        path: impl Into<String>,
        content: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        let path = path.into();
        Self {
            old_path: Some(DiffPath::new(path)),
            new_path: None,
            kind: DiffFileKind::Deleted,
            status,
            rows: content
                .as_ref()
                .lines()
                .enumerate()
                .map(|(index, text)| DiffRow {
                    old: Some(cell(Some(index + 1), text, DiffLineKind::Removed)),
                    new: None,
                })
                .collect(),
            metadata: DiffMetadata::default(),
        }
    }

    pub(super) fn modified(
        path: impl Into<String>,
        unified_diff: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        let path = path.into();
        let path = DiffPath::new(path);
        let rows = side_by_side_rows(unified_diff.as_ref());
        let metadata = if rows.is_empty() {
            DiffMetadata::from_rowless_modified_diff(unified_diff.as_ref())
        } else {
            DiffMetadata::from_diff(unified_diff.as_ref()).for_existing_file()
        };
        Self {
            old_path: Some(path.clone()),
            new_path: Some(path),
            kind: DiffFileKind::Modified,
            status,
            metadata,
            rows,
        }
    }

    pub(super) fn renamed(
        old_path: impl Into<String>,
        new_path: impl Into<String>,
        unified_diff: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        let old_path = old_path.into();
        let new_path = new_path.into();
        let rows = side_by_side_rows(unified_diff.as_ref());
        Self {
            old_path: Some(DiffPath::new(old_path)),
            new_path: Some(DiffPath::new(new_path)),
            kind: DiffFileKind::Renamed,
            status,
            metadata: DiffMetadata::from_diff(unified_diff.as_ref()).for_existing_file(),
            rows,
        }
    }

    pub(super) fn from_change(change: &FileUpdateChange, status: DiffStatus) -> Self {
        Self::from_change_with_diff(change, &change.diff, status)
    }

    pub(super) fn from_change_with_diff(
        change: &FileUpdateChange,
        diff: &str,
        status: DiffStatus,
    ) -> Self {
        let path = change.path.clone();
        match &change.kind {
            PatchChangeKind::Add => Self::added(path, diff, status),
            PatchChangeKind::Delete => Self::deleted(path, diff, status),
            PatchChangeKind::Update { move_path: None } => Self::modified(path, diff, status),
            PatchChangeKind::Update {
                move_path: Some(move_path),
            } => Self::renamed(path, move_path.to_string_lossy(), diff, status),
        }
    }

    pub(super) fn from_composed_parts(
        old_path: Option<DiffPath>,
        new_path: Option<DiffPath>,
        kind: DiffFileKind,
        status: DiffStatus,
        rows: Vec<DiffRow>,
        metadata: DiffMetadata,
    ) -> Self {
        Self {
            old_path,
            new_path,
            kind,
            status,
            rows,
            metadata,
        }
    }

    pub(super) fn display_path(&self) -> String {
        match (self.old_path(), self.new_path()) {
            (Some(old), Some(new)) if !old.equivalent(new) => {
                format!(
                    "{} -> {}",
                    bounded_visible_path(old.label()),
                    bounded_visible_path(new.label())
                )
            }
            (Some(path), _) | (_, Some(path)) => bounded_visible_path(path.label()),
            (None, None) => String::new(),
        }
    }

    pub(super) fn old_label(&self) -> Option<&str> {
        self.old_path.as_ref().map(DiffPath::label)
    }

    pub(super) fn new_label(&self) -> Option<&str> {
        self.new_path.as_ref().map(DiffPath::label)
    }

    pub(super) fn old_path(&self) -> Option<&DiffPath> {
        self.old_path.as_ref()
    }

    pub(super) fn new_path(&self) -> Option<&DiffPath> {
        self.new_path.as_ref()
    }

    pub(super) fn kind(&self) -> DiffFileKind {
        self.kind
    }

    pub(super) fn status(&self) -> DiffStatus {
        self.status
    }

    pub(super) fn rows(&self) -> &[DiffRow] {
        &self.rows
    }

    pub(super) fn metadata(&self) -> &DiffMetadata {
        &self.metadata
    }

    pub(super) fn stats(&self) -> DiffStats {
        DiffStats {
            files: 1,
            additions: self
                .rows
                .iter()
                .filter(|row| {
                    row.new
                        .as_ref()
                        .is_some_and(|cell| cell.kind == DiffLineKind::Added)
                })
                .count(),
            removals: self
                .rows
                .iter()
                .filter(|row| {
                    row.old
                        .as_ref()
                        .is_some_and(|cell| cell.kind == DiffLineKind::Removed)
                })
                .count(),
        }
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        [self.old_path(), self.new_path()]
            .into_iter()
            .flatten()
            .any(|path| {
                [other.old_path(), other.new_path()]
                    .into_iter()
                    .flatten()
                    .any(|other_path| path.equivalent(other_path))
            })
    }

    pub(super) fn same_identity(&self, other: &Self) -> bool {
        paths_are_equivalent(self.old_path(), other.old_path())
            && paths_are_equivalent(self.new_path(), other.new_path())
    }

    pub(super) fn rebase_display_root(&mut self, root: &Path) {
        for path in [&mut self.old_path, &mut self.new_path]
            .into_iter()
            .flatten()
        {
            path.rebase(root);
        }
    }

    pub(super) fn resolve_paths(&mut self, anchor: &Path, display_root: &Path) {
        for path in [&mut self.old_path, &mut self.new_path]
            .into_iter()
            .flatten()
        {
            path.resolve(anchor, display_root);
        }
    }

    pub(super) fn reroot_paths(&mut self, anchor: &Path, display_root: &Path) {
        for path in [&mut self.old_path, &mut self.new_path]
            .into_iter()
            .flatten()
        {
            path.reroot(anchor, display_root);
        }
    }

    pub(super) fn retained_text_bytes(&self) -> usize {
        self.old_path
            .as_ref()
            .map_or(0, DiffPath::retained_text_bytes)
            + self
                .new_path
                .as_ref()
                .map_or(0, DiffPath::retained_text_bytes)
            + self
                .rows
                .iter()
                .flat_map(|row| [row.old.as_ref(), row.new.as_ref()])
                .flatten()
                .map(|cell| cell.text.len())
                .sum::<usize>()
            + self.metadata.retained_text_bytes()
    }
}

fn paths_are_equivalent(left: Option<&DiffPath>, right: Option<&DiffPath>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.equivalent(right),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

pub(super) fn parse_unified_diff(unified_diff: &str) -> Vec<DiffFile> {
    let mut sections = Vec::new();
    let mut current = Vec::new();
    for line in unified_diff.lines() {
        if line.starts_with("diff --git ") && !current.is_empty() {
            sections.push(std::mem::take(&mut current));
        }
        current.push(line);
    }
    if !current.is_empty() {
        sections.push(current);
    }
    sections
        .into_iter()
        .filter_map(|section| parse_unified_section(&section))
        .collect()
}

fn parse_unified_section(lines: &[&str]) -> Option<DiffFile> {
    let git_paths = lines
        .iter()
        .find_map(|line| line.strip_prefix("diff --git ").and_then(parse_git_paths));
    let is_added = lines.iter().any(|line| line.starts_with("new file mode "));
    let is_deleted = lines
        .iter()
        .any(|line| line.starts_with("deleted file mode "));
    let rename_from = metadata_path(lines, "rename from ");
    let rename_to = metadata_path(lines, "rename to ");
    let copy_to = metadata_path(lines, "copy to ");
    let old_path = if is_added || copy_to.is_some() {
        None
    } else if rename_from.is_some() {
        rename_from
    } else {
        reconciled_header_path(
            lines,
            "--- ",
            git_paths.as_ref().map(|paths| paths.0.as_str()),
        )
        .unwrap_or_else(|| git_paths.as_ref().map(|paths| paths.0.clone()))
    };
    let new_path = if is_deleted {
        None
    } else if rename_to.is_some() || copy_to.is_some() {
        rename_to.or(copy_to)
    } else {
        reconciled_header_path(
            lines,
            "+++ ",
            git_paths.as_ref().map(|paths| paths.1.as_str()),
        )
        .unwrap_or_else(|| git_paths.as_ref().map(|paths| paths.1.clone()))
    };
    let status = DiffStatus::Completed;
    let body = lines.join("\n");
    match (old_path, new_path) {
        (None, Some(path)) => Some(DiffFile::added_from_unified(path, lines, status)),
        (Some(path), None) => Some(DiffFile::deleted_from_unified(path, lines, status)),
        (Some(old), Some(new)) if old != new => Some(DiffFile::renamed(old, new, body, status)),
        (Some(path), Some(_)) => Some(DiffFile::modified(path, body, status)),
        (None, None) => None,
    }
}

fn reconciled_header_path(
    lines: &[&str],
    prefix: &str,
    git_path: Option<&str>,
) -> Option<Option<String>> {
    match header_path(lines, prefix)? {
        Some(header_path) if git_path.is_some_and(|git_path| git_path != header_path) => {
            Some(git_path.map(str::to_string))
        }
        header_path => Some(header_path),
    }
}

impl DiffFile {
    fn added_from_unified(path: String, lines: &[&str], status: DiffStatus) -> Self {
        let unified_diff = lines.join("\n");
        let rows = side_by_side_rows(&unified_diff)
            .into_iter()
            .filter_map(|row| {
                let new = row.new?;
                (new.kind == DiffLineKind::Added).then_some(DiffRow {
                    old: None,
                    new: Some(new),
                })
            })
            .collect();
        Self {
            old_path: None,
            new_path: Some(DiffPath::new(path)),
            kind: DiffFileKind::Added,
            status,
            rows,
            metadata: DiffMetadata::from_diff(&unified_diff).for_added_file(),
        }
    }

    fn deleted_from_unified(path: String, lines: &[&str], status: DiffStatus) -> Self {
        let unified_diff = lines.join("\n");
        let rows = side_by_side_rows(&unified_diff)
            .into_iter()
            .filter_map(|row| {
                let old = row.old?;
                (old.kind == DiffLineKind::Removed).then_some(DiffRow {
                    old: Some(old),
                    new: None,
                })
            })
            .collect();
        Self {
            old_path: Some(DiffPath::new(path)),
            new_path: None,
            kind: DiffFileKind::Deleted,
            status,
            rows,
            metadata: DiffMetadata::from_diff(&unified_diff).for_deleted_file(),
        }
    }
}

fn side_by_side_rows(unified_diff: &str) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut old_line = 1;
    let mut new_line = 1;
    let mut in_hunk = false;
    let mut numbered_hunk = false;
    for line in unified_diff.lines() {
        if is_hunk_header(line) {
            flush_changes(&mut rows, &mut removed, &mut added);
            if let Some((old_start, new_start)) = hunk_starts(line) {
                (old_line, new_line, numbered_hunk) = (old_start, new_start, true);
            } else {
                (old_line, new_line, numbered_hunk) = (1, 1, false);
            }
            in_hunk = true;
            let hunk = cell(None, line, DiffLineKind::Hunk);
            rows.push(DiffRow {
                old: Some(hunk.clone()),
                new: Some(hunk),
            });
            continue;
        }
        if !in_hunk || line == "\\ No newline at end of file" {
            continue;
        }
        match line.as_bytes().first() {
            Some(b'-') => {
                removed.push(cell(
                    numbered_hunk.then_some(old_line),
                    &line[1..],
                    DiffLineKind::Removed,
                ));
                old_line += 1;
            }
            Some(b'+') => {
                added.push(cell(
                    numbered_hunk.then_some(new_line),
                    &line[1..],
                    DiffLineKind::Added,
                ));
                new_line += 1;
            }
            Some(b' ') => {
                flush_changes(&mut rows, &mut removed, &mut added);
                rows.push(DiffRow {
                    old: Some(cell(
                        numbered_hunk.then_some(old_line),
                        &line[1..],
                        DiffLineKind::Context,
                    )),
                    new: Some(cell(
                        numbered_hunk.then_some(new_line),
                        &line[1..],
                        DiffLineKind::Context,
                    )),
                });
                old_line += 1;
                new_line += 1;
            }
            _ => {}
        }
    }
    flush_changes(&mut rows, &mut removed, &mut added);
    rows
}

fn flush_changes(rows: &mut Vec<DiffRow>, removed: &mut Vec<DiffCell>, added: &mut Vec<DiffCell>) {
    for index in 0..removed.len().max(added.len()) {
        rows.push(DiffRow {
            old: removed.get(index).cloned(),
            new: added.get(index).cloned(),
        });
    }
    removed.clear();
    added.clear();
}

pub(super) fn is_hunk_header(line: &str) -> bool {
    hunk_starts(line).is_some()
        || line == "@@"
        || line.starts_with("@@ ") && !line.starts_with("@@ -")
}

pub(super) fn hunk_starts(line: &str) -> Option<(usize, usize)> {
    let ranges = line.strip_prefix("@@ -")?;
    let (old, rest) = ranges.split_once(" +")?;
    let new = rest.split_once(" @@").map_or(rest, |(range, _)| range);
    Some((range_start(old)?, range_start(new)?))
}

fn range_start(range: &str) -> Option<usize> {
    range.split(',').next()?.parse().ok()
}

fn cell(line_number: Option<usize>, text: impl Into<String>, kind: DiffLineKind) -> DiffCell {
    DiffCell {
        line_number,
        text: text.into(),
        kind,
    }
}
