use super::DashboardRoute;
use super::ShellState;
use super::composer::ComposerInsertResult;

impl ShellState {
    pub(super) fn insert_pasted_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        if text.is_empty()
            || self.diff_view.is_some()
            || self.tool_output.is_some()
            || self.agent_log.is_some()
            || self.selector.is_some()
            || self.command_palette.is_some()
            || self.safety_buffering_modal_lines().is_some()
            || self.pending_session_delete.is_some()
            || self.pending_approval.is_some()
        {
            return;
        }
        if let Some(state) = &mut self.pending_account_auth {
            state.insert_paste(&text);
            return;
        }
        if let Some(pending) = &self.pending_elicitation {
            if pending.editing() {
                self.insert_pasted_composer_text(&text);
            }
            return;
        }
        if self.pending_external_agent_import.is_some() {
            return;
        }
        if let Some(pending) = &mut self.pending_mcp_management {
            if pending.editing() {
                pending.push_draft(&text);
            }
            return;
        }
        if self.pending_plugin_management.is_some() {
            return;
        }
        if self.pending_user_input.is_some() {
            self.insert_pasted_composer_text(&text);
            return;
        }
        if self.dashboard_route == DashboardRoute::Sessions && self.session_list.focused {
            if self.session_list.renaming() {
                self.session_list.insert_rename_text(&text);
            } else if self.session_list.search_active() {
                self.session_list.insert_search_text(&text);
            }
            return;
        }
        if self.dashboard_route == DashboardRoute::Status && self.settings.focused {
            if self.settings.editing() {
                self.settings.insert_edit_text(&text);
            }
            return;
        }
        if self.dashboard_focused() {
            return;
        }
        self.insert_pasted_composer_text(&text);
    }

    fn insert_pasted_composer_text(&mut self, text: &str) {
        self.slash_command_popup.reset();
        let result = self.composer.insert_str(text);
        if result == ComposerInsertResult::Inserted {
            self.clear_transcript_selection();
            self.clear_transcript_text_selection();
        }
        self.report_composer_insert(result);
    }
}
