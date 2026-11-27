use super::ShellState;
use super::backend::AppShellBackend;
use super::slash_commands::GoalSlashCommand;
use crate::goal_display::GOAL_USAGE;
use crate::goal_display::goal_status_label;
use crate::goal_display::goal_usage_summary;
use codex_app_server_protocol::ThreadGoalStatus;

impl ShellState {
    pub(super) async fn run_goal_slash_command<S>(
        &mut self,
        command: GoalSlashCommand,
        app_server: &mut S,
    ) where
        S: AppShellBackend,
    {
        match command {
            GoalSlashCommand::Set(objective) => {
                self.set_goal_objective(app_server, objective).await
            }
            GoalSlashCommand::Clear => self.clear_goal(app_server).await,
            GoalSlashCommand::Pause => {
                self.update_goal_status(app_server, ThreadGoalStatus::Paused, "paused")
                    .await;
            }
            GoalSlashCommand::Resume => {
                self.update_goal_status(app_server, ThreadGoalStatus::Active, "resumed")
                    .await;
            }
            GoalSlashCommand::Show | GoalSlashCommand::Edit => {
                match app_server.thread_goal_get(self.thread_id).await {
                    Ok(response) => {
                        self.record_active_goal(response.goal);
                        match &self.active_goal {
                            Some(goal) if command == GoalSlashCommand::Edit => {
                                self.composer.set_text(format!("/goal {}", goal.objective));
                                self.request_external_editor();
                            }
                            Some(goal) => self.show_command_report(
                                "Goal",
                                format!(
                                    "{}\n\n{}",
                                    goal_status_label(goal.status),
                                    goal_usage_summary(goal)
                                ),
                            ),
                            None => self.show_command_report(
                                "Goal",
                                format!("No active goal.\n\n{GOAL_USAGE}"),
                            ),
                        }
                    }
                    Err(error) => {
                        self.command_feedback("Goal", format!("Could not read goal: {error}"))
                    }
                }
            }
        }
    }

    pub(super) async fn set_goal_objective<S>(&mut self, app_server: &mut S, objective: String)
    where
        S: AppShellBackend,
    {
        match app_server
            .thread_goal_set(
                self.thread_id,
                Some(objective),
                Some(ThreadGoalStatus::Active),
                /*token_budget*/ None,
            )
            .await
        {
            Ok(response) => {
                self.record_active_goal(Some(response.goal));
                self.show_hint("Goal set");
            }
            Err(err) => self.command_feedback("Goal", format!("failed to set goal: {err}")),
        }
    }

    async fn clear_goal<S>(&mut self, app_server: &mut S)
    where
        S: AppShellBackend,
    {
        match app_server.thread_goal_clear(self.thread_id).await {
            Ok(response) => {
                self.record_active_goal(None);
                if response.cleared {
                    self.show_hint("goal cleared");
                } else {
                    self.show_hint("no goal is currently set");
                }
            }
            Err(err) => self.command_feedback("Goal", format!("failed to clear goal: {err}")),
        }
    }

    async fn update_goal_status<S>(
        &mut self,
        app_server: &mut S,
        status: ThreadGoalStatus,
        action: &str,
    ) where
        S: AppShellBackend,
    {
        match app_server
            .thread_goal_set(
                self.thread_id,
                /*objective*/ None,
                Some(status),
                /*token_budget*/ None,
            )
            .await
        {
            Ok(response) => {
                self.record_active_goal(Some(response.goal));
                self.show_hint(format!("goal {action}"));
            }
            Err(err) => self.command_feedback("Goal", format!("failed to update goal: {err}")),
        }
    }
}
