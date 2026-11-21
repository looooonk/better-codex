//! Lightweight incremental token estimates for streamed model output.

/// Estimates a fragment using the last character of the preceding fragment.
pub fn estimate_tokens(text: &str, previous: &mut char) -> f64 {
    let mut tokens = 0.0;
    for c in text.chars() {
        tokens += token_weight(c, *previous);
        *previous = c;
    }
    tokens
}

// Calibrated against o200k_base counts on source code, Markdown, and translated UI text.
// A word start (also "V" in "HeaderView") usually costs more than a letter within a word.
fn token_weight(c: char, previous: char) -> f64 {
    let weight = match c {
        'a'..='z' | 'A'..='Z' => 0.05,
        '0'..='9' if previous.is_ascii_digit() => 0.34,
        '0'..='9' => 1.1,
        '\n' => 0.85,
        _ if c.is_whitespace() => return if previous.is_whitespace() { 0.04 } else { 0.0 },
        _ if c.is_ascii() => 0.54,
        '\u{0080}'..='\u{024f}' => 0.5, // Latin-1 and Latin Extended A/B.
        '\u{0250}'..='\u{07ff}' => 0.18, // Includes Greek, Cyrillic, Hebrew and Arabic.
        '\u{0900}'..='\u{0dff}' => 0.45, // Indic scripts.
        _ if c.len_utf8() == 3 => 0.72,
        _ => 1.75,
    };
    if c.len_utf8() <= 2
        && c.is_alphabetic()
        && (!previous.is_alphabetic() || previous.is_ascii_lowercase() && c.is_ascii_uppercase())
    {
        weight + 0.85
    } else {
        weight
    }
}
