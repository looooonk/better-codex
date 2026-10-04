use super::ShellState;
use crate::inline_visualization::InlineVisualizationContext;
use codex_protocol::ThreadId;
use codex_protocol::models::PermissionProfile;
use codex_utils_absolute_path::AbsolutePathBuf;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct VisualizationState {
    scope: Option<Scope>,
    context: Option<InlineVisualizationContext>,
}

#[derive(PartialEq)]
struct Scope {
    thread_id: ThreadId,
    codex_home: PathBuf,
    cwd: String,
    permissions: PermissionProfile,
    additional_roots: Vec<AbsolutePathBuf>,
    remote: bool,
}

impl VisualizationState {
    pub(super) fn context(&mut self, shell: &ShellState) -> Option<&InlineVisualizationContext> {
        let scope = Scope {
            thread_id: shell.thread_id,
            codex_home: shell.codex_home.clone(),
            cwd: shell.cwd.clone(),
            permissions: shell.permission_profile.clone(),
            additional_roots: shell.runtime_workspace_roots.clone(),
            remote: shell
                .resume_cwd_runtime
                .uses_remote_workspace_or_environment,
        };
        if self.scope.as_ref() != Some(&scope) {
            self.context = if scope.remote {
                None
            } else {
                InlineVisualizationContext::from_session(
                    &scope.codex_home,
                    scope.thread_id,
                    Path::new(&scope.cwd),
                    &scope.permissions,
                    &scope.additional_roots,
                )
            };
            self.scope = Some(scope);
        }
        self.context.as_ref()
    }
}

#[cfg(test)]
#[path = "inline_visualizations_tests.rs"]
mod tests;
