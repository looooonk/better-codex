use super::*;
use crate::app_shell::render::ShellView;
use crate::app_shell::slash_commands::LocalSlashCommand;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn review_arguments_select_native_review_targets() {
    assert_eq!(
        [
            review_target(""),
            review_target("--base main"),
            review_target("--commit abc123"),
            review_target("check concurrency")
        ]
        .map(Result::unwrap),
        [
            ReviewTarget::UncommittedChanges,
            ReviewTarget::BaseBranch {
                branch: "main".to_string()
            },
            ReviewTarget::Commit {
                sha: "abc123".to_string(),
                title: None
            },
            ReviewTarget::Custom {
                instructions: "check concurrency".to_string()
            },
        ]
    );
    assert!(review_target("--base").is_err());
    assert!(review_target("--commit one two").is_err());
}

#[test]
fn workspace_commands_keep_arguments_local() {
    assert_eq!(
        LocalSlashCommand::parse(" /experimental future_feature on "),
        Some(LocalSlashCommand::Workspace(
            WorkspaceCommand::Experimental,
            "future_feature on".to_string()
        ))
    );
    let shell = ShellState::snapshot_fixture();
    assert_eq!(
        shell
            .workspace_request(WorkspaceCommand::Experimental, "future_feature on")
            .unwrap(),
        WorkspaceRequest::Experimental(Some(("future_feature".to_string(), true)))
    );
    assert!(
        shell
            .workspace_request(WorkspaceCommand::Experimental, "feature maybe")
            .is_err()
    );
}

#[test]
fn plan_toggle_preserves_selected_model_and_effort() {
    let mut shell = ShellState::snapshot_fixture();
    shell.model = "gpt-6.1-sol".to_string();
    shell.reasoning_effort = Some(codex_protocol::openai_models::ReasoningEffort::High);
    let expected = CollaborationMode {
        mode: ModeKind::Plan,
        settings: Settings {
            model: shell.model.clone(),
            reasoning_effort: shell.reasoning_effort.clone(),
            developer_instructions: None,
        },
    };
    assert_eq!(
        shell.workspace_request(WorkspaceCommand::Plan, "").unwrap(),
        WorkspaceRequest::Plan(expected.clone())
    );
    shell.collaboration_mode = Some(Box::new(expected.clone()));
    let mut stale = expected.clone();
    stale.settings.model = "previous-model".to_string();
    shell.complete_workspace_request(shell.thread_id, Ok(WorkspaceResponse::Mode(stale)));
    assert_eq!(shell.collaboration_mode.as_deref(), Some(&expected));
    assert_eq!(
        shell.workspace_request(WorkspaceCommand::Plan, "").unwrap(),
        WorkspaceRequest::Plan(CollaborationMode {
            mode: ModeKind::Default,
            ..expected
        })
    );
}

#[test]
fn feature_output_stays_with_its_session() {
    let mut shell = ShellState::snapshot_fixture();
    let previous = shell.transcript.len();
    shell.complete_workspace_request(
        ThreadId::new(),
        Ok(WorkspaceResponse::Notice("old session".to_string())),
    );
    assert_eq!(shell.transcript.len(), previous);
}

#[test]
fn workspace_review_command_completion_snapshot() {
    let mut shell = ShellState::snapshot_fixture();
    shell.dashboard_visible = false;
    shell.composer.set_text("/rev".to_string());
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 22,
    );
    let mut buffer = Buffer::empty(area);
    ShellView { shell: &shell }.render(area, &mut buffer);
    let lines = buffer
        .content
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>();
    insta::assert_snapshot!(lines.join("\n"));
}

#[test]
fn usage_reset_requires_explicit_confirmation() {
    let shell = ShellState::snapshot_fixture();
    for args in ["", "reset"] {
        assert_eq!(
            shell
                .workspace_request(WorkspaceCommand::Usage, args)
                .unwrap(),
            WorkspaceRequest::Extension(
                crate::app_shell::extension_commands::ExtensionCommand::Usage { reset: false }
            )
        );
    }
    assert_eq!(
        shell
            .workspace_request(WorkspaceCommand::Usage, "reset confirm")
            .unwrap(),
        WorkspaceRequest::Extension(
            crate::app_shell::extension_commands::ExtensionCommand::Usage { reset: true }
        )
    );
}

#[test]
fn completed_workspace_request_clears_only_its_loading_status() {
    let mut shell = ShellState::snapshot_fixture();
    shell.active_turn_id = None;
    for (before, expected) in [
        ("loading workspace action", "ready"),
        ("loading diagnostics", "ready"),
        ("interrupted", "interrupted"),
    ] {
        shell.status = before.to_string();
        shell.complete_workspace_request(
            shell.thread_id,
            Ok(WorkspaceResponse::Notice("done".to_string())),
        );
        assert_eq!(shell.status, expected);
    }
}
