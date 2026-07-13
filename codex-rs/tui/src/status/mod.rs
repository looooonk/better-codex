use dirs::home_dir;
use std::path::Path;
use unicode_width::UnicodeWidthStr;

pub(crate) fn format_tokens_compact(value: i64) -> String {
    let value = value.max(0);
    if value < 1_000 {
        return value.to_string();
    }

    let value = value as f64;
    let (scaled, suffix) = if value >= 1_000_000_000_000.0 {
        (value / 1_000_000_000_000.0, "T")
    } else if value >= 1_000_000_000.0 {
        (value / 1_000_000_000.0, "B")
    } else if value >= 1_000_000.0 {
        (value / 1_000_000.0, "M")
    } else {
        (value / 1_000.0, "K")
    };
    let decimals = if scaled < 10.0 {
        2
    } else if scaled < 100.0 {
        1
    } else {
        0
    };
    let mut formatted = format!("{scaled:.decimals$}");
    while formatted.contains('.') && formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    format!("{formatted}{suffix}")
}

pub(crate) fn format_directory_display(directory: &Path, max_width: Option<usize>) -> String {
    let formatted = home_dir()
        .and_then(|home| directory.strip_prefix(home).ok().map(Path::to_path_buf))
        .map_or_else(
            || directory.display().to_string(),
            |relative| {
                if relative.as_os_str().is_empty() {
                    "~".to_string()
                } else {
                    format!("~{}{}", std::path::MAIN_SEPARATOR, relative.display())
                }
            },
        );

    match max_width {
        Some(0) => String::new(),
        Some(max_width) if UnicodeWidthStr::width(formatted.as_str()) > max_width => {
            crate::text_formatting::center_truncate_path(&formatted, max_width)
        }
        Some(_) | None => formatted,
    }
}
