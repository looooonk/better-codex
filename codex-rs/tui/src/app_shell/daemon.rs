use super::ShellState;
use crate::DaemonUpdateSource;
use crate::UpdateAction;

impl ShellState {
    pub(super) fn run_daemon_command(&mut self, args: &str) {
        if !self.resume_cwd_runtime.daemon_update_available {
            self.show_hint("daemon updates require a connection to the local background server");
            return;
        }
        let source = match args {
            "" | "status" => {
                self.push_system("Connected to the local background server. Use /daemon update latest to install the latest Better Codex release, or /daemon update from-cli to use this CLI. Updating closes this interface and restarts the server.".to_string());
                return;
            }
            "update latest" => DaemonUpdateSource::PublicStable,
            "update from-cli" => DaemonUpdateSource::ThisCli,
            _ => {
                self.show_hint("usage: /daemon [status|update latest|update from-cli]");
                return;
            }
        };
        if self.active_turn_id.is_some()
            || self.has_pending_backend_actions()
            || self.has_pending_shell_command()
            || self.composer.has_queued_messages()
        {
            self.show_hint("finish active work before updating the background server");
            return;
        }
        self.pending_update_action = Some(UpdateAction::Daemon(source));
    }
}

#[cfg(test)]
#[path = "daemon_tests.rs"]
mod tests;
