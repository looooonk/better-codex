use super::ShellState;
use super::agent_activity::AgentActivityState;
use super::backend::AppShellBackend;
use super::backend::ThreadRehydration;
use super::backend_actions::ActionGroup;
use crate::token_usage::TokenUsage;
use codex_app_server_protocol::ThreadStatus;
use codex_app_server_protocol::TurnStatus;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RevertedProjectionRefresh {
    Revert,
    LagRecovery,
}

impl ShellState {
    pub(super) async fn handle_current_thread_reverted<S>(&mut self, app_server: &S)
    where
        S: AppShellBackend,
    {
        self.refresh_reverted_session_projection(app_server, RevertedProjectionRefresh::Revert)
            .await;
    }

    pub(super) async fn handle_reverted_thread_lag<S>(&mut self, app_server: &S) -> bool
    where
        S: AppShellBackend,
    {
        if !self.reverted_thread_refresh_enabled() {
            return false;
        }
        let active_agent_thread_ids = self.active_agent_thread_ids.clone();
        let subscribed_agent_thread_ids = self
            .agent_history_task
            .as_ref()
            .map(crate::app_server_session::AgentHistoryTask::subscribed_thread_ids)
            .unwrap_or_default();
        let agent_activity = self.agent_activity.clone();
        let pending_approval = self.pending_approval.take();
        let pending_elicitation = self.pending_elicitation.take();
        let pending_user_input = self.pending_user_input.take();
        let queued_interactive_requests = std::mem::take(&mut self.queued_interactive_requests);
        let safety_buffering = std::mem::take(&mut self.safety_buffering);
        let streaming_assistant = std::mem::take(&mut self.streaming_assistant);
        let streaming_assistant_item_id = self.streaming_assistant_item_id.take();
        let streaming_assistant_revision = self.streaming_assistant_revision;
        let streaming_plan = std::mem::take(&mut self.streaming_plan);
        let streaming_plan_item_id = self.streaming_plan_item_id.take();
        let streaming_plan_revision = self.streaming_plan_revision;
        let plan_explanation = self.plan_explanation.take();
        let plan_steps = std::mem::take(&mut self.plan_steps);
        let pending_prompt_submission = self.pending_prompt_submission.take();
        let pending_vim_input = self.pending_vim_input.take();
        let pending_session_delete = self.pending_session_delete.take();
        self.refresh_reverted_session_projection(
            app_server,
            RevertedProjectionRefresh::LagRecovery,
        )
        .await;
        self.active_agent_thread_ids.extend(active_agent_thread_ids);
        self.active_agent_thread_ids
            .extend(subscribed_agent_thread_ids);
        self.agent_activity = agent_activity;
        self.pending_approval = pending_approval;
        self.pending_elicitation = pending_elicitation;
        self.pending_user_input = pending_user_input;
        self.queued_interactive_requests = queued_interactive_requests;
        self.safety_buffering = safety_buffering;
        self.streaming_assistant = streaming_assistant;
        self.streaming_assistant_item_id = streaming_assistant_item_id;
        self.streaming_assistant_revision = streaming_assistant_revision;
        self.streaming_plan = streaming_plan;
        self.streaming_plan_item_id = streaming_plan_item_id;
        self.streaming_plan_revision = streaming_plan_revision;
        self.plan_explanation = plan_explanation;
        self.plan_steps = plan_steps;
        self.pending_prompt_submission = pending_prompt_submission;
        self.pending_vim_input = pending_vim_input;
        self.pending_session_delete = pending_session_delete;
        true
    }

    async fn refresh_reverted_session_projection<S>(
        &mut self,
        app_server: &S,
        refresh: RevertedProjectionRefresh,
    ) where
        S: AppShellBackend,
    {
        self.finish_subscription_cleanup().await;
        self.cancel_agent_history().await;
        let previous_thread_ids = self.tracked_thread_ids();
        self.invalidate_session_hydration();
        self.reset_reverted_conversation_projection();
        if refresh == RevertedProjectionRefresh::Revert {
            self.composer = self.composer.clone_without_queue();
            self.queue_state.reset();
            self.backend_actions.invalidate([
                ActionGroup::Approval,
                ActionGroup::Compaction,
                ActionGroup::ConversationBranch,
                ActionGroup::TurnStart,
                ActionGroup::UserInput,
                ActionGroup::QueueHydration,
                ActionGroup::QueueMutation,
            ]);
        }
        self.record_active_goal(None);
        self.token_usage = TokenUsage::default();
        self.context_token_usage = TokenUsage::default();
        self.model_context_window = None;
        self.thread_usage = None;
        if refresh == RevertedProjectionRefresh::Revert {
            self.pending_prompt_submission = None;
            self.pending_vim_input = None;
        }
        self.status = "refreshing reverted session".to_string();
        self.begin_thread_revert_hydration(app_server, previous_thread_ids);
        self.start_replaced_session_hydration(app_server);
        self.request_queue_hydration(app_server);
    }

    pub(super) fn apply_reverted_thread<S>(
        &mut self,
        app_server: &S,
        rehydration: ThreadRehydration,
    ) where
        S: AppShellBackend,
    {
        let ThreadRehydration {
            thread,
            agent_history_task,
        } = rehydration;
        let thread_id = thread.id;
        let thread_status = thread.status;
        let active_turn_id = thread
            .turns
            .iter()
            .rev()
            .find(|turn| turn.status == TurnStatus::InProgress)
            .map(|turn| turn.id.clone());
        let live_active_turn_id = self.active_turn_id.clone();
        let pending_approval = self.pending_approval.take();
        let pending_elicitation = self.pending_elicitation.take();
        let pending_user_input = self.pending_user_input.take();
        let queued_interactive_requests = std::mem::take(&mut self.queued_interactive_requests);
        let live_transcript = std::mem::take(&mut self.transcript);
        let live_tool_activity = std::mem::take(&mut self.tool_activity);
        let live_subagent_activity = std::mem::take(&mut self.subagent_activity);
        let live_agent_thread_ids = std::mem::take(&mut self.active_agent_thread_ids);
        let live_agent_activity = self.agent_activity.clone();
        let safety_buffering = std::mem::take(&mut self.safety_buffering);
        let streaming_assistant = std::mem::take(&mut self.streaming_assistant);
        let streaming_assistant_item_id = self.streaming_assistant_item_id.take();
        let streaming_assistant_revision = self.streaming_assistant_revision;
        let streaming_plan = std::mem::take(&mut self.streaming_plan);
        let streaming_plan_item_id = self.streaming_plan_item_id.take();
        let streaming_plan_revision = self.streaming_plan_revision;
        let plan_explanation = self.plan_explanation.take();
        let plan_steps = std::mem::take(&mut self.plan_steps);
        self.thread_name = thread.name;
        self.cwd = thread.cwd.to_string_lossy().to_string();
        self.reset_reverted_conversation_projection();
        self.agent_activity = live_agent_activity;
        self.diff_store.set_display_root(thread.cwd.as_path());
        self.push_system("session reverted");
        self.ingest_turn_history(thread.turns);
        for line in live_transcript {
            let already_hydrated = line.item_id.as_ref().is_some_and(|item_id| {
                line.kind == super::TranscriptKind::Output
                    && self.transcript.iter().any(|hydrated| {
                        hydrated.kind == super::TranscriptKind::Output
                            && hydrated.item_id.as_ref() == Some(item_id)
                    })
            });
            if !already_hydrated {
                self.upsert_line(line);
            }
        }
        for activity in live_tool_activity {
            self.upsert_tool_activity(activity.id, activity.title, activity.status);
        }
        for activity in live_subagent_activity {
            self.upsert_subagent_activity(activity.id, activity.title, activity.status);
        }
        self.active_agent_thread_ids.extend(live_agent_thread_ids);
        self.install_agent_history(Vec::new(), agent_history_task);
        let cleanup_thread_ids =
            std::mem::take(&mut self.session_hydration.thread_revert_cleanup_thread_ids);
        self.prepare_replaced_session_cleanup(app_server, cleanup_thread_ids);
        self.pending_approval = pending_approval;
        self.pending_elicitation = pending_elicitation;
        self.pending_user_input = pending_user_input;
        self.queued_interactive_requests = queued_interactive_requests;
        self.safety_buffering = safety_buffering;
        self.streaming_assistant = streaming_assistant;
        self.streaming_assistant_item_id = streaming_assistant_item_id;
        self.streaming_assistant_revision = streaming_assistant_revision;
        self.streaming_plan = streaming_plan;
        self.streaming_plan_item_id = streaming_plan_item_id;
        self.streaming_plan_revision = streaming_plan_revision;
        if plan_explanation.is_some() || !plan_steps.is_empty() {
            self.plan_explanation = plan_explanation;
            self.plan_steps = plan_steps;
        }
        let status_tracks_active_turn = matches!(&thread_status, ThreadStatus::Active { .. });
        self.handle_remote_thread_status(&thread_id, thread_status);
        if let Some(turn_id) = active_turn_id.or(live_active_turn_id) {
            self.record_active_turn_started(turn_id);
            if !status_tracks_active_turn {
                self.status = "thinking".to_string();
            }
        }
    }

    fn reset_reverted_conversation_projection(&mut self) {
        self.close_agent_log();
        self.close_tool_output();
        self.close_diff_view();
        self.clear_text_selections();
        self.transcript.clear();
        self.transcript_scroll = 0;
        self.transcript_scroll_max.set(0);
        self.transcript_selection = None;
        self.transcript_render_cache.get_mut().clear();
        self.clear_streaming_transcript();
        self.plan_explanation = None;
        self.plan_steps.clear();
        self.tool_activity.clear();
        self.agent_activity = AgentActivityState::for_root(self.thread_id.to_string());
        self.active_agent_thread_ids.clear();
        self.deferred_unsubscribe_thread_ids.clear();
        self.subagent_activity.clear();
        self.latest_diff = None;
        self.diff_store.clear();
        self.diff_store
            .set_display_root(std::path::Path::new(&self.cwd));
        self.rewind = super::rewind::RewindState::default();
        self.clear_active_turn();
        self.clear_interactive_requests();
        self.pending_session_delete = None;
        self.selector = None;
        self.command_palette = None;
        self.safety_buffering.clear();
    }
}
