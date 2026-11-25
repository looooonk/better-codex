use super::*;
use pretty_assertions::assert_eq;

#[test]
fn command_output_compaction_retains_the_low_water_line_tail() {
    let output = (0..=TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|line| format!("compile line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let retained_from = TRANSCRIPT_OUTPUT_HIGH_WATER_LINES
        .saturating_add(1)
        .saturating_sub(TRANSCRIPT_OUTPUT_LOW_WATER_LINES);
    let retained = (retained_from..=TRANSCRIPT_OUTPUT_HIGH_WATER_LINES)
        .map(|line| format!("compile line {line:03}"))
        .collect::<Vec<_>>()
        .join("\n");

    assert_eq!(
        compact_output_for_transcript(&output),
        format!("{TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX}{retained}")
    );
}

#[test]
fn command_output_compaction_waits_for_the_high_water_mark_after_trimming() {
    let oversized = "a".repeat(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS + 1);
    let compacted = compact_output_for_transcript(&oversized);
    let retained = compacted
        .strip_prefix(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX)
        .expect("oversized output should be marked as compacted");
    assert_eq!(retained.chars().count(), TRANSCRIPT_OUTPUT_LOW_WATER_CHARS);

    let delta = "b".repeat(TRANSCRIPT_OUTPUT_HIGH_WATER_CHARS - TRANSCRIPT_OUTPUT_LOW_WATER_CHARS);
    let at_high_water = format!("{compacted}{delta}");
    assert_eq!(compact_output_for_transcript(&at_high_water), at_high_water);

    let compacted_again = compact_output_for_transcript(&format!("{at_high_water}c"));
    let retained_again = compacted_again
        .strip_prefix(TRANSCRIPT_OUTPUT_TRUNCATION_PREFIX)
        .expect("output above the high water mark should be compacted again");
    assert_eq!(
        retained_again.chars().count(),
        TRANSCRIPT_OUTPUT_LOW_WATER_CHARS
    );
    assert!(retained_again.ends_with('c'));
}
