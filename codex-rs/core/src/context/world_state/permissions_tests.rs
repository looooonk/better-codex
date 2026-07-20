use super::*;
use crate::context::world_state::WorldState;
use codex_protocol::config_types::ApprovalsReviewer;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::openai_models::ApprovalMessages;
use codex_protocol::openai_models::PermissionMessages;
use pretty_assertions::assert_eq;
use std::path::Path;

#[test]
fn snapshot_hashes_the_full_rendered_permissions_fragment() {
    let before = permissions_state("Ask before running this command.");
    let after = permissions_state("Ask before executing this command.");
    let before_snapshot = before.snapshot();

    assert_ne!(before_snapshot, after.snapshot());
    assert_eq!(
        after
            .render_diff(PreviousSectionState::Known(&before_snapshot))
            .map(|fragment| fragment.render()),
        Some(after.instructions.render()),
    );
    let after_snapshot = after.snapshot();
    assert!(
        after
            .render_diff(PreviousSectionState::Known(&after_snapshot))
            .is_none()
    );

    let retained: ResponseItem = ContextualUserFragment::into(after.instructions.clone());
    let mut bundled_retained = retained.clone();
    let ResponseItem::Message { content, .. } = &mut bundled_retained else {
        panic!("permissions should render as a message");
    };
    content.insert(
        0,
        ContentItem::InputText {
            text: "Other developer instructions.".to_string(),
        },
    );
    let mut world_state = WorldState::default();
    world_state.add_section(after);
    let snapshot = world_state.snapshot();

    assert!(
        world_state
            .render_history_diff(/*previous*/ None, &[retained])
            .is_empty()
    );
    assert_eq!(
        world_state.render_history_diff(Some(&snapshot), &[]).len(),
        1
    );
    assert!(
        world_state
            .render_history_diff(Some(&snapshot), &[bundled_retained])
            .is_empty()
    );
}

fn permissions_state(on_request: &str) -> PermissionsState {
    let approval_messages = ApprovalMessages {
        on_request: Some(on_request.to_string()),
        on_request_auto_review: None,
    };
    let permission_messages = PermissionMessages {
        danger_full_access: Some("Full access.".to_string()),
        workspace_write: Some("Workspace write.".to_string()),
        read_only: Some("Read only.".to_string()),
    };
    PermissionsState::new(
        &PermissionProfile::read_only(),
        AskForApproval::OnRequest,
        ApprovalPromptContext::new(
            ApprovalsReviewer::User,
            Some(&approval_messages),
            Some(&permission_messages),
        ),
        &Policy::empty(),
        Path::new("/workspace"),
        /*exec_permission_approvals_enabled*/ false,
        /*request_permissions_tool_enabled*/ false,
    )
}
