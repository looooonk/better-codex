//! Test-only helpers shared across the TUI crate.

use std::fmt::Write;
use std::sync::LazyLock;

use codex_models_manager::bundled_models_response;
use codex_protocol::openai_models::ModelPreset;
pub(crate) use codex_utils_absolute_path::test_support::PathBufExt;
pub(crate) use codex_utils_absolute_path::test_support::test_path_buf;
use ratatui::buffer::Buffer;

pub(crate) static TEST_MODEL_PRESETS: LazyLock<Vec<ModelPreset>> = LazyLock::new(|| {
    let mut response = bundled_models_response()
        .unwrap_or_else(|err| panic!("bundled models.json should parse: {err}"));
    response.models.sort_by_key(|model| model.priority);
    let mut presets: Vec<ModelPreset> = response.models.into_iter().map(Into::into).collect();
    ModelPreset::mark_default_by_picker_visibility(&mut presets);
    presets
});

pub(crate) fn buffer_style_grid(buffer: &Buffer) -> String {
    const STYLE_IDS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

    let mut styles = Vec::new();
    let mut counts = Vec::new();
    let mut style_indices = Vec::with_capacity(buffer.content().len());
    for cell in buffer.content() {
        let style = cell.style();
        let index = match styles.iter().position(|candidate| *candidate == style) {
            Some(index) => index,
            None => {
                styles.push(style);
                counts.push(0);
                styles.len() - 1
            }
        };
        counts[index] += 1;
        style_indices.push(index);
    }
    assert!(
        styles.len() <= STYLE_IDS.len(),
        "style grid supports at most {} distinct styles, found {}",
        STYLE_IDS.len(),
        styles.len()
    );

    let width = usize::from(buffer.area.width);
    let mut snapshot = format!("style grid {}x{}:\n", buffer.area.width, buffer.area.height);
    for row in style_indices.chunks(width.max(1)) {
        for &index in row {
            snapshot.push(char::from(STYLE_IDS[index]));
        }
        snapshot.push('\n');
    }
    snapshot.push_str("legend:\n");
    for (index, (style, count)) in styles.iter().zip(counts).enumerate() {
        writeln!(
            snapshot,
            "{} ({count} cells): {style:?}",
            char::from(STYLE_IDS[index])
        )
        .expect("writing a style snapshot to a String should succeed");
    }
    snapshot
}
