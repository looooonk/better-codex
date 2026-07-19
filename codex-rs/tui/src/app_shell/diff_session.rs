use super::diff_model::DiffCell;
use super::diff_model::DiffFile;
use super::diff_model::DiffFileKind;
use super::diff_model::DiffLineKind;
use super::diff_model::DiffRow;
use super::diff_model::DiffStatus;
use super::diff_model::hunk_starts;

const MAX_SESSION_COMPOSE_LINES: usize = 8_000;

pub(super) fn compose_session_files(files: Vec<DiffFile>) -> Vec<DiffFile> {
    let mut histories = Vec::<SessionFileHistory>::new();
    for file in files {
        let source = file.old_label().or_else(|| file.new_label());
        if let Some(history) = histories
            .iter_mut()
            .find(|history| history.current_path.as_deref() == source)
        {
            history.apply(&file);
        } else {
            histories.push(SessionFileHistory::new(&file));
        }
    }
    histories
        .into_iter()
        .filter_map(SessionFileHistory::finish)
        .collect()
}

struct SessionFileHistory {
    original_path: Option<String>,
    current_path: Option<String>,
    original_exists: bool,
    current_exists: bool,
    baseline: Vec<Option<String>>,
    current: Vec<TrackedLine>,
    status: DiffStatus,
    latest_file: DiffFile,
    applied_files: usize,
    line_offset: usize,
    composable: bool,
}

struct TrackedLine {
    origin: Option<usize>,
    text: Option<String>,
}

impl SessionFileHistory {
    fn new(file: &DiffFile) -> Self {
        let original_exists = file.kind() != DiffFileKind::Added;
        let original_path = original_exists
            .then(|| file.old_label().or_else(|| file.new_label()))
            .flatten()
            .map(str::to_owned);
        let mut history = Self {
            current_path: original_path.clone(),
            original_path,
            original_exists,
            current_exists: original_exists,
            baseline: Vec::new(),
            current: Vec::new(),
            status: file.status(),
            latest_file: file.clone(),
            applied_files: 0,
            line_offset: 0,
            composable: true,
        };
        history.apply(file);
        history
    }

    fn apply(&mut self, file: &DiffFile) {
        self.applied_files += 1;
        self.latest_file = file.clone();
        if file.kind() == DiffFileKind::Added && !self.current_exists {
            self.current.clear();
        }
        if self.composable {
            self.apply_rows(file.rows());
        }
        self.status = file.status();
        match file.kind() {
            DiffFileKind::Added | DiffFileKind::Modified | DiffFileKind::Renamed => {
                self.current_exists = true;
                self.current_path = file
                    .new_label()
                    .or_else(|| file.old_label())
                    .map(str::to_owned);
            }
            DiffFileKind::Deleted => {
                self.current_exists = false;
                self.current.clear();
            }
        }
    }

    fn apply_rows(&mut self, rows: &[DiffRow]) {
        let mut position = None;
        let mut batch = Vec::new();
        for row in rows {
            if !self.composable {
                return;
            }
            if let Some(hunk) = row
                .old
                .as_ref()
                .filter(|cell| cell.kind == DiffLineKind::Hunk)
            {
                self.apply_batch(position, &batch);
                batch.clear();
                position = hunk_starts(&hunk.text).map(|(_, new)| new.saturating_sub(1));
            } else {
                batch.push(row);
            }
        }
        self.apply_batch(position, &batch);
    }

    fn apply_batch(&mut self, position: Option<usize>, rows: &[&DiffRow]) {
        if rows.is_empty() {
            return;
        }
        let position = position.unwrap_or_else(|| {
            rows.iter()
                .find_map(|row| {
                    row.new
                        .as_ref()
                        .and_then(|cell| cell.line_number)
                        .or_else(|| row.old.as_ref().and_then(|cell| cell.line_number))
                })
                .unwrap_or(1)
                .saturating_sub(1)
        });
        let consumed = rows.iter().filter(|row| row.old.is_some()).count();
        let position = self.local_position(position);
        if !self.composable {
            return;
        }
        let Some(required_len) = position
            .checked_add(consumed)
            .filter(|len| *len <= MAX_SESSION_COMPOSE_LINES)
        else {
            self.composable = false;
            return;
        };
        let produced = rows.iter().filter(|row| row.new.is_some()).count();
        let Some(_) = self
            .current
            .len()
            .max(required_len)
            .checked_sub(consumed)
            .and_then(|len| len.checked_add(produced))
            .filter(|len| *len <= MAX_SESSION_COMPOSE_LINES)
        else {
            self.composable = false;
            return;
        };
        self.ensure_current_len(required_len);
        let mut cursor = position;
        for row in rows {
            let removed = row.old.as_ref().map(|old| {
                let mut line = self.current.remove(cursor);
                line.text = Some(old.text.clone());
                if let Some(origin) = line.origin
                    && self.baseline[origin].is_none()
                {
                    self.baseline[origin] = Some(old.text.clone());
                }
                line
            });
            if let Some(new) = &row.new {
                self.current.insert(
                    cursor,
                    TrackedLine {
                        origin: removed.as_ref().and_then(|line| line.origin),
                        text: Some(new.text.clone()),
                    },
                );
                cursor += 1;
            }
        }
    }

    fn local_position(&mut self, position: usize) -> usize {
        if self.baseline.is_empty() && self.current.is_empty() {
            self.line_offset = position;
            return 0;
        }
        if position >= self.line_offset {
            return position - self.line_offset;
        }
        let prefix = self.line_offset - position;
        if prefix.saturating_add(self.baseline.len().max(self.current.len()))
            > MAX_SESSION_COMPOSE_LINES
        {
            self.composable = false;
            return 0;
        }
        for line in &mut self.current {
            if let Some(origin) = &mut line.origin {
                *origin += prefix;
            }
        }
        self.baseline
            .splice(0..0, std::iter::repeat_n(None, prefix));
        self.current.splice(
            0..0,
            (0..prefix).map(|origin| TrackedLine {
                origin: Some(origin),
                text: None,
            }),
        );
        self.line_offset = position;
        0
    }

    fn ensure_current_len(&mut self, len: usize) {
        while self.current.len() < len {
            let origin = self.baseline.len();
            self.baseline.push(None);
            self.current.push(TrackedLine {
                origin: Some(origin),
                text: None,
            });
        }
    }

    fn finish(self) -> Option<DiffFile> {
        if self.applied_files == 1 || !self.composable {
            return Some(self.latest_file);
        }
        if !self.original_exists && !self.current_exists {
            return None;
        }
        let rows = composed_rows(&self.baseline, &self.current, self.line_offset);
        let renamed = self.original_path != self.current_path;
        if rows.is_empty() && !renamed {
            return None;
        }
        let kind = match (self.original_exists, self.current_exists, renamed) {
            (false, true, _) => DiffFileKind::Added,
            (true, false, _) => DiffFileKind::Deleted,
            (true, true, true) => DiffFileKind::Renamed,
            (true, true, false) => DiffFileKind::Modified,
            (false, false, _) => return None,
        };
        Some(DiffFile::from_composed_parts(
            self.original_path,
            self.current_path,
            kind,
            self.status,
            rows,
        ))
    }
}

#[derive(Default)]
struct ChangeGroup {
    old_start: usize,
    new_start: usize,
    old: Vec<String>,
    new: Vec<String>,
}

fn composed_rows(
    baseline: &[Option<String>],
    current: &[TrackedLine],
    line_offset: usize,
) -> Vec<DiffRow> {
    let mut groups = Vec::new();
    let mut group = ChangeGroup::default();
    let mut old = 0usize;
    let mut new = 0usize;
    while old < baseline.len() || new < current.len() {
        let old_line = line_number(line_offset, old);
        let new_line = line_number(line_offset, new);
        match current.get(new).and_then(|line| line.origin) {
            Some(origin) if old < origin => {
                push_old(&mut group, baseline[old].clone(), old_line, new_line);
                old += 1;
            }
            Some(origin) if old == origin => {
                let old_text = baseline[old].as_deref();
                let new_text = current[new].text.as_deref();
                if old_text == new_text || old_text.is_none() || new_text.is_none() {
                    flush_group(&mut groups, &mut group);
                } else {
                    start_group(&mut group, old_line, new_line);
                    group.old.push(old_text.unwrap_or_default().to_string());
                    group.new.push(new_text.unwrap_or_default().to_string());
                }
                old += 1;
                new += 1;
            }
            Some(_) => {
                flush_group(&mut groups, &mut group);
                new += 1;
            }
            None if new < current.len() => {
                push_new(&mut group, current[new].text.clone(), old_line, new_line);
                new += 1;
            }
            None => {
                push_old(&mut group, baseline[old].clone(), old_line, new_line);
                old += 1;
            }
        }
    }
    flush_group(&mut groups, &mut group);
    groups.into_iter().flat_map(group_rows).collect()
}

fn line_number(line_offset: usize, index: usize) -> usize {
    line_offset.saturating_add(index).saturating_add(1)
}

fn start_group(group: &mut ChangeGroup, old_start: usize, new_start: usize) {
    if group.old.is_empty() && group.new.is_empty() {
        group.old_start = old_start;
        group.new_start = new_start;
    }
}

fn push_old(group: &mut ChangeGroup, text: Option<String>, old: usize, new: usize) {
    if let Some(text) = text {
        start_group(group, old, new);
        group.old.push(text);
    }
}

fn push_new(group: &mut ChangeGroup, text: Option<String>, old: usize, new: usize) {
    if let Some(text) = text {
        start_group(group, old, new);
        group.new.push(text);
    }
}

fn flush_group(groups: &mut Vec<ChangeGroup>, group: &mut ChangeGroup) {
    if !group.old.is_empty() || !group.new.is_empty() {
        groups.push(std::mem::take(group));
    }
}

fn group_rows(group: ChangeGroup) -> Vec<DiffRow> {
    let old_count = group.old.len();
    let new_count = group.new.len();
    let old_hunk_start = if old_count == 0 {
        group.old_start.saturating_sub(1)
    } else {
        group.old_start
    };
    let new_hunk_start = if new_count == 0 {
        group.new_start.saturating_sub(1)
    } else {
        group.new_start
    };
    let hunk = DiffCell {
        line_number: None,
        text: format!("@@ -{old_hunk_start},{old_count} +{new_hunk_start},{new_count} @@"),
        kind: DiffLineKind::Hunk,
    };
    let mut rows = vec![DiffRow {
        old: Some(hunk.clone()),
        new: Some(hunk),
    }];
    for index in 0..old_count.max(new_count) {
        rows.push(DiffRow {
            old: group.old.get(index).map(|text| DiffCell {
                line_number: Some(group.old_start + index),
                text: text.clone(),
                kind: DiffLineKind::Removed,
            }),
            new: group.new.get(index).map(|text| DiffCell {
                line_number: Some(group.new_start + index),
                text: text.clone(),
                kind: DiffLineKind::Added,
            }),
        });
    }
    rows
}
