use codex_app_server_protocol::FileUpdateChange;
use codex_app_server_protocol::PatchApplyStatus;
use codex_app_server_protocol::PatchChangeKind;

const MAX_DIFF_PATH_BYTES: usize = 1_024;

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
    old_path: Option<String>,
    new_path: Option<String>,
    kind: DiffFileKind,
    status: DiffStatus,
    rows: Vec<DiffRow>,
}

impl DiffFile {
    pub(super) fn added(
        path: impl Into<String>,
        content: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        Self {
            old_path: None,
            new_path: Some(path.into()),
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
        }
    }

    pub(super) fn deleted(
        path: impl Into<String>,
        content: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        Self {
            old_path: Some(path.into()),
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
        }
    }

    pub(super) fn modified(
        path: impl Into<String>,
        unified_diff: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        let path = path.into();
        Self {
            old_path: Some(path.clone()),
            new_path: Some(path),
            kind: DiffFileKind::Modified,
            status,
            rows: side_by_side_rows(unified_diff.as_ref()),
        }
    }

    pub(super) fn renamed(
        old_path: impl Into<String>,
        new_path: impl Into<String>,
        unified_diff: impl AsRef<str>,
        status: DiffStatus,
    ) -> Self {
        Self {
            old_path: Some(old_path.into()),
            new_path: Some(new_path.into()),
            kind: DiffFileKind::Renamed,
            status,
            rows: side_by_side_rows(unified_diff.as_ref()),
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
        let path = bounded_path(&change.path);
        match &change.kind {
            PatchChangeKind::Add => Self::added(path, diff, status),
            PatchChangeKind::Delete => Self::deleted(path, diff, status),
            PatchChangeKind::Update { move_path: None } => Self::modified(path, diff, status),
            PatchChangeKind::Update {
                move_path: Some(move_path),
            } => Self::renamed(
                path,
                bounded_path(&move_path.to_string_lossy()),
                diff,
                status,
            ),
        }
    }

    pub(super) fn display_path(&self) -> String {
        match (self.old_label(), self.new_label()) {
            (Some(old), Some(new)) if old != new => format!("{old} -> {new}"),
            (Some(path), _) | (_, Some(path)) => path.to_string(),
            (None, None) => String::new(),
        }
    }

    pub(super) fn old_label(&self) -> Option<&str> {
        self.old_path.as_deref()
    }

    pub(super) fn new_label(&self) -> Option<&str> {
        self.new_path.as_deref()
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

    pub(super) fn identity(&self) -> (Option<&str>, Option<&str>) {
        (self.old_label(), self.new_label())
    }

    pub(super) fn retained_text_bytes(&self) -> usize {
        self.old_path.as_ref().map_or(0, String::len)
            + self.new_path.as_ref().map_or(0, String::len)
            + self
                .rows
                .iter()
                .flat_map(|row| [row.old.as_ref(), row.new.as_ref()])
                .flatten()
                .map(|cell| cell.text.len())
                .sum::<usize>()
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
    let old_path = header_path(lines, "--- ")
        .unwrap_or_else(|| git_paths.as_ref().map(|paths| paths.0.clone()));
    let new_path = header_path(lines, "+++ ")
        .unwrap_or_else(|| git_paths.as_ref().map(|paths| paths.1.clone()));
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

impl DiffFile {
    fn added_from_unified(path: String, lines: &[&str], status: DiffStatus) -> Self {
        let rows = side_by_side_rows(&lines.join("\n"))
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
            new_path: Some(path),
            kind: DiffFileKind::Added,
            status,
            rows,
        }
    }

    fn deleted_from_unified(path: String, lines: &[&str], status: DiffStatus) -> Self {
        let rows = side_by_side_rows(&lines.join("\n"))
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
            old_path: Some(path),
            new_path: None,
            kind: DiffFileKind::Deleted,
            status,
            rows,
        }
    }
}

fn header_path(lines: &[&str], prefix: &str) -> Option<Option<String>> {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(prefix).map(normalize_diff_path))
}

fn side_by_side_rows(unified_diff: &str) -> Vec<DiffRow> {
    let mut rows = Vec::new();
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let mut old_line = 1;
    let mut new_line = 1;
    let mut in_hunk = false;
    for line in unified_diff.lines() {
        if let Some((old_start, new_start)) = hunk_starts(line) {
            flush_changes(&mut rows, &mut removed, &mut added);
            (old_line, new_line, in_hunk) = (old_start, new_start, true);
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
                removed.push(cell(Some(old_line), &line[1..], DiffLineKind::Removed));
                old_line += 1;
            }
            Some(b'+') => {
                added.push(cell(Some(new_line), &line[1..], DiffLineKind::Added));
                new_line += 1;
            }
            Some(b' ') => {
                flush_changes(&mut rows, &mut removed, &mut added);
                rows.push(DiffRow {
                    old: Some(cell(Some(old_line), &line[1..], DiffLineKind::Context)),
                    new: Some(cell(Some(new_line), &line[1..], DiffLineKind::Context)),
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

fn hunk_starts(line: &str) -> Option<(usize, usize)> {
    if line == "@@" {
        return Some((1, 1));
    }
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

fn normalize_diff_path(path: &str) -> Option<String> {
    let path = path.trim().trim_matches('"');
    (path != "/dev/null").then(|| {
        bounded_path(
            path.strip_prefix("a/")
                .or_else(|| path.strip_prefix("b/"))
                .unwrap_or(path),
        )
    })
}

fn bounded_path(path: &str) -> String {
    if path.len() <= MAX_DIFF_PATH_BYTES {
        return path.to_string();
    }
    let mut end = MAX_DIFF_PATH_BYTES.saturating_sub(3);
    while !path.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &path[..end])
}

fn parse_git_paths(paths: &str) -> Option<(String, String)> {
    let (old, rest) = diff_path_token(paths)?;
    let (new, _) = diff_path_token(rest)?;
    Some((normalize_diff_path(&old)?, normalize_diff_path(&new)?))
}

fn diff_path_token(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    if let Some(quoted) = input.strip_prefix('"') {
        let end = quoted.find('"')?;
        return Some((quoted[..end].replace("\\\"", "\""), &quoted[end + 1..]));
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    Some((input[..end].to_string(), &input[end..]))
}
