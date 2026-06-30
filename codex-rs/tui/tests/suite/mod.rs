// Aggregates all former standalone integration tests as modules.
mod resize_reflow;
mod status_indicator;
#[cfg(unix)]
mod terminal_restore;
mod vt100_history;
mod vt100_live_commit;
