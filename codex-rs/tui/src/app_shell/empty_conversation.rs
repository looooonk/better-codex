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
        "█   █ █         █ ",
        "█ ▄ █ █▀▄  ▀▀▄ ▀█▀",
        "█▄▀▄█ █ █ ▄▀▀█  █ ",
        "▀   ▀ ▀ ▀  ▀▀▀  ▀▀",
    ],
    [
        // should
        "     █           █    █",
        "▄▀▀▀ █▀▄ ▄▀▄ █ █ █  ▄▀█",
        " ▀▀▄ █ █ █ █ █ █ █  █ █",
        "▀▀▀  ▀ ▀  ▀   ▀▀  ▀  ▀▀",
    ],
    [
        // we
        "          ",
        "█   █ ▄▀▀▄",
        "█ █ █ █▀▀▀",
        " ▀ ▀   ▀▀▀",
    ],
    [
        // build?
        "█       ▀ █    █ ▄▀▀▄",
        "█▀▄ █ █ █ █  ▄▀█   ▄▀",
        "█ █ █ █ █ █  █ █  ▀  ",
        "▀▀   ▀▀ ▀  ▀  ▀▀  ▀  ",
    ],
];

pub(super) fn render(area: Rect, buf: &mut Buffer) {
    if area.is_empty() {
        return;
    }
    // On narrow panes pack two bitmap columns into one quadrant-block cell.
    let wide_width = WORDS
        .iter()
        .map(|word| word[0].chars().count())
        .sum::<usize>()
        + 3 * (WORDS.len() - 1);
    let compact = area.width < 44 || (area.height < 9 && wide_width > usize::from(area.width));
    let words = WORDS.map(|rows| {
        rows.map(|row| {
            if compact {
                condense(row)
            } else {
                row.to_owned()
            }
        })
    });
    let gap = if compact { "  " } else { "   " };
    let width_of = |word: &[String; 4]| word[0].chars().count();
    let phrase_width = words.iter().map(width_of).sum::<usize>() + gap.len() * (words.len() - 1);
    let mut lines = Vec::new();
    let mut remaining = words.as_slice();
    while let Some((first, rest)) = remaining.split_first() {
        let mut width = width_of(first);
        let mut count = 1;
        // Prefer “What should” / “we build?” when the phrase needs two lines.
        let limit = if remaining.len() == words.len() && phrase_width > usize::from(area.width) {
            2
        } else {
            remaining.len()
        };
        for word in rest.iter().take(limit - 1) {
            let next = width + gap.len() + width_of(word);
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
                remaining[..count]
                    .iter()
                    .map(|word| word[row].as_str())
                    .collect::<Vec<_>>()
                    .join(gap),
            );
        }
        remaining = &remaining[count..];
    }
    let height = lines.len() as u16;
    let (text, height) = if height <= area.height
        && words
            .iter()
            .all(|word| width_of(word) <= usize::from(area.width))
    {
        (lines.join("\n"), height)
    } else {
        let lines = textwrap::wrap("What should we build?", usize::from(area.width));
        (lines.join("\n"), lines.len() as u16)
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

fn condense(row: &str) -> String {
    // Each row of the table is a left half-block, each column a right half-block.
    const CELLS: [[char; 4]; 4] = [
        [' ', '▝', '▗', '▐'],
        ['▘', '▀', '▚', '▜'],
        ['▖', '▞', '▄', '▟'],
        ['▌', '▛', '▙', '█'],
    ];
    let mut pixels = row.chars().map(|cell| match cell {
        '▀' => 1,
        '▄' => 2,
        '█' => 3,
        _ => 0,
    });
    let mut line = String::new();
    while let Some(left) = pixels.next() {
        line.push(CELLS[left][pixels.next().unwrap_or(0)]);
    }
    line
}
