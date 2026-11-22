use super::design::palette;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;

// Each cell contains two vertical pixels. Keep words separate for narrow panes.
const WORDS: [[&str; 4]; 4] = [
    [
        // What
        "█   █ █        ▄ ",
        "█ ▄ █ █▄  ▄▄  ▄█▄",
        "█▄▀▄█ █ █ ▄▄█  █ ",
        "▀   ▀ ▀ ▀ ▀▀▀  ▀▀",
    ],
    [
        // should
        "    █           █    █",
        " ▄▄ █▄   ▄  ▄ ▄ █   ▄█",
        "▀▀▄ █ █ █ █ █ █ █  █ █",
        "▀▀  ▀ ▀  ▀   ▀▀  ▀  ▀▀",
    ],
    [
        // we
        "         ",
        "▄   ▄  ▄ ",
        "█ █ █ █▄▀",
        " ▀ ▀   ▀▀",
    ],
    [
        // build?
        "█       ▀ █    █ ▄▀▀▄",
        "█▄  ▄ ▄ ▄ █   ▄█   ▄▀",
        "█ █ █ █ █ █  █ █  ▀  ",
        "▀▀   ▀▀ ▀  ▀  ▀▀  ▀  ",
    ],
];

pub(super) fn render(area: Rect, buf: &mut Buffer) {
    let mut lines = Vec::new();
    let mut words = WORDS.as_slice();
    while let Some((first, rest)) = words.split_first() {
        let mut width = first[0].chars().count();
        let mut count = 1;
        for word in rest {
            let next = width + 3 + word[0].chars().count();
            if next > usize::from(area.width) {
                break;
            }
            count += 1;
            width = next;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        for row in 0..first.len() {
            lines.push(
                words[..count]
                    .iter()
                    .map(|word| word[row])
                    .collect::<Vec<_>>()
                    .join("   "),
            );
        }
        words = &words[count..];
    }
    // Keep the complete prompt readable when the terminal is too short for the bitmap.
    let height = lines.len() as u16;
    let (text, height) = if height <= area.height {
        (lines.join("\n"), height)
    } else {
        ("What should we build?".to_string(), 1)
    };
    let centered = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(height) / 2,
        area.width,
        height.min(area.height),
    );
    Paragraph::new(text)
        .centered()
        .fg(palette::text())
        .render(centered, buf);
}
