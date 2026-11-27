use super::ShellState;
use std::collections::VecDeque;

#[derive(Default)]
pub(super) struct WarningHistory(VecDeque<String>);

impl WarningHistory {
    pub(super) fn record(&mut self, message: &str) {
        let mut retained = message.chars().take(16_384).collect::<String>();
        if retained.len() < message.len() {
            retained.push_str("\n[warning truncated]");
        }
        if self.0.back() != Some(&retained) {
            self.0.push_back(retained);
            if self.0.len() > 100 {
                self.0.pop_front();
            }
        }
    }
}

impl ShellState {
    pub(super) fn retain_warning(&mut self, message: String) {
        self.warnings.record(&message);
        self.show_hint(message.lines().next().unwrap_or("warning"));
    }

    pub(super) fn show_warnings(&mut self) {
        if self.warnings.0.is_empty() {
            self.show_hint("No retained warnings");
        } else {
            self.show_command_report(
                "Warnings",
                self.warnings
                    .0
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            );
        }
    }
}

#[cfg(test)]
#[path = "diagnostic_commands_tests.rs"]
mod tests;
