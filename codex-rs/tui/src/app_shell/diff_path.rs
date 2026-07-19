const MAX_DIFF_PATH_BYTES: usize = 1_024;

pub(super) fn header_path(lines: &[&str], prefix: &str) -> Option<Option<String>> {
    lines.iter().find_map(|line| {
        line.strip_prefix(prefix)
            .map(|path| diff_path_token(path).and_then(|(path, _)| normalize_diff_path(&path)))
    })
}

pub(super) fn bounded_path(path: &str) -> String {
    if path.len() <= MAX_DIFF_PATH_BYTES {
        return path.to_string();
    }
    let mut end = MAX_DIFF_PATH_BYTES.saturating_sub(3);
    while !path.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}...", &path[..end])
}

pub(super) fn parse_git_paths(paths: &str) -> Option<(String, String)> {
    let (old, rest) = diff_path_token(paths)?;
    let (new, _) = diff_path_token(rest)?;
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

fn normalize_diff_path(path: &str) -> Option<String> {
    let path = path.trim();
    (path != "/dev/null").then(|| {
        bounded_path(
            path.strip_prefix("a/")
                .or_else(|| path.strip_prefix("b/"))
                .unwrap_or(path),
        )
    })
}

fn diff_path_token(input: &str) -> Option<(String, &str)> {
    let input = input.trim_start();
    if input.starts_with('"') {
        return quoted_diff_path(input);
    }
    let end = input.find(char::is_whitespace).unwrap_or(input.len());
    Some((input[..end].to_string(), &input[end..]))
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
