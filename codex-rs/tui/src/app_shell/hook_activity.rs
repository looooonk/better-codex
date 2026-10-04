use super::ShellState;
use super::ToolBlockStatus;
use codex_app_server_protocol::HookRunStatus;
use codex_app_server_protocol::HookRunSummary;

impl ShellState {
    pub(super) fn record_hook_activity(&mut self, run: HookRunSummary) {
        let status = match run.status {
            HookRunStatus::Running => ToolBlockStatus::Running,
            HookRunStatus::Completed => ToolBlockStatus::Success,
            HookRunStatus::Failed | HookRunStatus::Blocked | HookRunStatus::Stopped => {
                ToolBlockStatus::Fail
            }
        };
        let title = format!("Hook: {:?}", run.event_name);
        self.push_tool_with_status_for_item(run.id.clone(), title, status);
        let output = run
            .status_message
            .into_iter()
            .chain(run.entries.into_iter().map(|entry| entry.text))
            .collect::<Vec<_>>()
            .join("\n");
        if !output.is_empty() {
            self.push_output_with_status_for_item(run.id, output, status);
        }
    }
}
