use std::collections::hash_map::DefaultHasher;
use std::hash::Hash;
use std::hash::Hasher;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

const MAX_DIFF_PATH_BYTES: usize = 1_024;
const MAX_DIFF_IDENTITY_BYTES: usize = 16 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffPath {
    identity: String,
    label: String,
    relative_source: Option<String>,
}

impl DiffPath {
    pub(super) fn new(path: impl AsRef<str>) -> Self {
        let identity = bounded_identity(&lexical_path(path.as_ref()));
        Self {
            label: identity.clone(),
            relative_source: (!Path::new(&identity).is_absolute()).then(|| identity.clone()),
            identity,
        }
    }

    pub(super) fn label(&self) -> &str {
        &self.label
    }

    pub(super) fn equivalent(&self, other: &Self) -> bool {
        if self.identity == other.identity {
            return true;
        }
        Path::new(&self.identity).is_absolute() != Path::new(&other.identity).is_absolute()
            && self.label == other.label
    }

    pub(super) fn possible_namespace_variant(&self, other: &Self) -> bool {
        fn has_one_leading_component(longer: &str, shorter: &str) -> bool {
            let longer = Path::new(longer);
            let shorter = Path::new(shorter);
            if longer.is_absolute() || shorter.is_absolute() {
                return false;
            }
            let mut longer = longer.components();
            matches!(longer.next(), Some(Component::Normal(_))) && longer.eq(shorter.components())
        }

        has_one_leading_component(&self.label, &other.label)
            || has_one_leading_component(&other.label, &self.label)
    }

    pub(super) fn resolve(&mut self, anchor: &Path, display_root: &Path) {
        if !Path::new(&self.identity).is_absolute() {
            self.identity = bounded_identity(&lexical_path(
                &anchor.join(&self.identity).to_string_lossy(),
            ));
        }
        self.rebase(display_root);
    }

    pub(super) fn reroot(&mut self, anchor: &Path, display_root: &Path) {
        if let Some(relative_source) = &self.relative_source {
            self.identity = bounded_identity(&lexical_path(
                &anchor.join(relative_source).to_string_lossy(),
            ));
        }
        self.rebase(display_root);
    }

    pub(super) fn rebase(&mut self, display_root: &Path) {
        self.label =
            relative_label(&self.identity, display_root).unwrap_or_else(|| self.identity.clone());
    }

    pub(super) fn retained_text_bytes(&self) -> usize {
        self.identity
            .len()
            .saturating_add(self.label.len())
            .saturating_add(self.relative_source.as_ref().map_or(0, String::len))
    }
}

pub(super) fn header_path(lines: &[&str], prefix: &str) -> Option<Option<String>> {
    lines.iter().find_map(|line| {
        line.strip_prefix(prefix)
            .map(|path| header_path_value(path).and_then(|path| normalize_diff_path(&path)))
    })
}

pub(super) fn metadata_path(lines: &[&str], prefix: &str) -> Option<String> {
    lines
        .iter()
        .find_map(|line| line.strip_prefix(prefix).and_then(header_path_value))
}

pub(super) fn bounded_path(path: &str) -> String {
    bounded_path_with_limit(path, MAX_DIFF_PATH_BYTES)
}

fn bounded_identity(path: &str) -> String {
    bounded_path_with_limit(path, MAX_DIFF_IDENTITY_BYTES)
}

fn bounded_path_with_limit(path: &str, max_bytes: usize) -> String {
    if path.len() <= max_bytes {
        return path.to_string();
    }
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    let marker = format!("...{:016x}...", hasher.finish());
    let retained = max_bytes.saturating_sub(marker.len());
    let mut prefix_end = retained / 2;
    while !path.is_char_boundary(prefix_end) {
        prefix_end = prefix_end.saturating_sub(1);
    }
    let mut suffix_start = path
        .len()
        .saturating_sub(retained.saturating_sub(prefix_end));
    while !path.is_char_boundary(suffix_start) {
        suffix_start = suffix_start.saturating_add(1);
    }
    format!("{}{}{}", &path[..prefix_end], marker, &path[suffix_start..])
}

pub(super) fn parse_git_paths(paths: &str) -> Option<(String, String)> {
    let (old, new) = git_path_pair(paths)?;
    Some((normalize_diff_path(&old)?, normalize_diff_path(&new)?))
}

pub(super) fn visible_path(path: &str) -> String {
    path.chars().fold(String::new(), |mut visible, ch| {
        if ch.is_control() {
            visible.extend(ch.escape_default());
        } else {
            visible.push(ch);
        }
        visible
    })
}

pub(super) fn bounded_visible_path(path: &str) -> String {
    bounded_path(&visible_path(path))
}

fn normalize_diff_path(path: &str) -> Option<String> {
    (path != "/dev/null").then(|| {
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path)
            .to_string()
    })
}

fn lexical_path(path: &str) -> String {
    let mut prefix = None;
    let mut rooted = false;
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Prefix(value) => {
                prefix = Some(value.as_os_str().to_string_lossy().into_owned());
            }
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.last().is_some_and(|part| part != "..") {
                    parts.pop();
                } else if !rooted {
                    parts.push("..".to_string());
                }
            }
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
        }
    }
    let body = parts.join("/");
    match (prefix, rooted, body.is_empty()) {
        (Some(prefix), true, false) => format!("{prefix}/{body}"),
        (Some(prefix), true, true) => format!("{prefix}/"),
        (Some(prefix), false, false) => format!("{prefix}{body}"),
        (Some(prefix), false, true) => prefix,
        (None, true, false) => format!("/{body}"),
        (None, true, true) => "/".to_string(),
        (None, false, _) => body,
    }
}

fn relative_label(identity: &str, root: &Path) -> Option<String> {
    if !Path::new(identity).is_absolute() {
        return None;
    }
    let root = lexical_path(&root.to_string_lossy());
    let relative = Path::new(identity).strip_prefix(PathBuf::from(root)).ok()?;
    let label = relative.to_string_lossy();
    (!label.is_empty()).then(|| label.into_owned())
}

fn header_path_value(input: &str) -> Option<String> {
    if input.starts_with('"') {
        return quoted_diff_path(input).map(|(path, _)| path);
    }
    Some(match input.rsplit_once('\t') {
        Some((path, timestamp)) if timestamp.is_empty() || looks_like_diff_timestamp(timestamp) => {
            path.to_string()
        }
        Some(_) | None => input.to_string(),
    })
}

fn looks_like_diff_timestamp(value: &str) -> bool {
    let mut parts = value.split_ascii_whitespace();
    let Some(date) = parts.next() else {
        return false;
    };
    let Some(time) = parts.next() else {
        return false;
    };
    let timezone = parts.next();
    if parts.next().is_some() {
        return false;
    }
    let date = date.as_bytes();
    let time = time.as_bytes();
    date.len() == 10
        && date[..4].iter().all(u8::is_ascii_digit)
        && date[4] == b'-'
        && date[5..7].iter().all(u8::is_ascii_digit)
        && date[7] == b'-'
        && date[8..].iter().all(u8::is_ascii_digit)
        && time.len() >= 8
        && time[..2].iter().all(u8::is_ascii_digit)
        && time[2] == b':'
        && time[3..5].iter().all(u8::is_ascii_digit)
        && time[5] == b':'
        && time[6..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || *byte == b'.')
        && timezone.is_none_or(|timezone| {
            let timezone = timezone.as_bytes();
            timezone.len() == 5
                && matches!(timezone[0], b'+' | b'-')
                && timezone[1..].iter().all(u8::is_ascii_digit)
        })
}

fn git_path_pair(input: &str) -> Option<(String, String)> {
    let input = input.trim_start();
    if input.starts_with('"') {
        let (old, rest) = quoted_diff_path(input)?;
        return Some((old, trailing_git_path(rest)?));
    }

    if let Some(boundary) = input.find(" \"") {
        let old = input[..boundary].to_string();
        let (new, _) = quoted_diff_path(&input[boundary + 1..])?;
        return Some((old, new));
    }

    if let Some(boundary) = unquoted_git_path_boundary(input) {
        return Some((
            input[..boundary].to_string(),
            input[boundary + 1..].to_string(),
        ));
    }

    let boundary = input.find(char::is_whitespace)?;
    let new = input[boundary..].trim_start();
    (!new.is_empty()).then(|| (input[..boundary].to_string(), new.to_string()))
}

fn trailing_git_path(input: &str) -> Option<String> {
    let input = input.trim_start();
    if input.starts_with('"') {
        quoted_diff_path(input).map(|(path, _)| path)
    } else {
        (!input.is_empty()).then(|| input.to_string())
    }
}

fn unquoted_git_path_boundary(input: &str) -> Option<usize> {
    let mut candidates = input.match_indices(" b/").map(|(index, _)| index);
    let first = candidates.next()?;
    let mut best = first;
    let mut best_score = path_pair_score(input, first);
    for candidate in candidates {
        let score = path_pair_score(input, candidate);
        if score < best_score {
            best = candidate;
            best_score = score;
        }
    }
    Some(best)
}

fn path_pair_score(input: &str, boundary: usize) -> (bool, usize) {
    let old = strip_diff_prefix(&input[..boundary]);
    let new = strip_diff_prefix(&input[boundary + 1..]);
    (old != new, old.len().abs_diff(new.len()))
}

fn strip_diff_prefix(path: &str) -> &str {
    path.strip_prefix("a/")
        .or_else(|| path.strip_prefix("b/"))
        .unwrap_or(path)
}

fn quoted_diff_path(input: &str) -> Option<(String, &str)> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 1usize;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                return Some((
                    String::from_utf8_lossy(&decoded).into_owned(),
                    &input[index + 1..],
                ));
            }
            b'\\' => {
                index += 1;
                let escaped = *bytes.get(index)?;
                if matches!(escaped, b'0'..=b'7') {
                    let mut value = 0u8;
                    for _ in 0..3 {
                        let digit = *bytes.get(index)?;
                        if !matches!(digit, b'0'..=b'7') {
                            break;
                        }
                        value = value.wrapping_mul(8).wrapping_add(digit - b'0');
                        index += 1;
                    }
                    decoded.push(value);
                    continue;
                }
                decoded.push(match escaped {
                    b'a' => b'\x07',
                    b'b' => b'\x08',
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'v' => b'\x0b',
                    b'f' => b'\x0c',
                    b'r' => b'\r',
                    escaped => escaped,
                });
            }
            byte => decoded.push(byte),
        }
        index += 1;
    }
    None
}

#[cfg(test)]
#[path = "diff_path_tests.rs"]
mod tests;
