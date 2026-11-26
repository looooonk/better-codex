use super::ShellState;
use super::dashboard::dashboard_value;
use super::dashboard::format_usize;
use super::design::palette;
use crate::status::format_directory_display;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_READ_ONLY;
use codex_protocol::models::BUILT_IN_PERMISSION_PROFILE_WORKSPACE;
use codex_protocol::models::PermissionProfile;
use ratatui::style::Stylize;
use ratatui::text::Line;
use std::path::Path;

pub(super) fn workspace_lines(shell: &ShellState, width: usize) -> Vec<Line<'static>> {
    let mut workspace_lines = Vec::new();
    if let Some(git_status) = &shell.workspace_git_status {
        if git_status.is_dirty() {
            workspace_lines.push(Line::from(format!(
                "changes {} paths",
                format_usize(git_status.changes.total())
            )));
            workspace_lines.extend(workspace_change_lines(&git_status.changes));
        } else {
            workspace_lines.push(Line::from("tree clean".fg(palette::success())));
        }
        if let Some(branch) = &git_status.branch {
            workspace_lines.push(Line::from(vec![
                "branch ".dim(),
                dashboard_value(branch, width, /*prefix_width*/ 7).fg(palette::cyan()),
            ]));
        }
    }
    workspace_lines.push(Line::from(vec![
        "profile ".dim(),
        profile_label(shell).into(),
    ]));
    let cwd = format_directory_display(Path::new(&shell.cwd), /*max_width*/ None);
    workspace_lines.push(Line::from(vec![
        "cwd ".dim(),
        dashboard_value(&cwd, width, /*prefix_width*/ 4).into(),
    ]));
    if shell.runtime_workspace_roots.is_empty() {
        workspace_lines.push(Line::from("roots none selected".dim()));
    } else if shell.runtime_workspace_roots.len() > 1 {
        const WORKSPACE_ROOT_PREVIEW_LIMIT: usize = 3;
        let root_count = shell.runtime_workspace_roots.len();
        workspace_lines.push(Line::from(format!(
            "roots {} writable",
            format_usize(root_count)
        )));
        for root in shell
            .runtime_workspace_roots
            .iter()
            .take(WORKSPACE_ROOT_PREVIEW_LIMIT)
        {
            workspace_lines.push(Line::from(vec![
                "  ".dim(),
                dashboard_value(&root.display().to_string(), width, /*prefix_width*/ 2).dim(),
            ]));
        }
        let hidden = root_count.saturating_sub(WORKSPACE_ROOT_PREVIEW_LIMIT);
        if hidden > 0 {
            workspace_lines.push(Line::from(
                format!("  +{} more", format_usize(hidden)).dim(),
            ));
        }
    }
    workspace_lines
}

fn profile_label(shell: &ShellState) -> &'static str {
    if let Some(active) = &shell.active_permission_profile {
        return match active.id.as_str() {
            BUILT_IN_PERMISSION_PROFILE_DANGER_FULL_ACCESS => "full access",
            BUILT_IN_PERMISSION_PROFILE_READ_ONLY => "read only",
            BUILT_IN_PERMISSION_PROFILE_WORKSPACE => "workspace write",
            _ => "custom",
        };
    }
    match &shell.permission_profile {
        PermissionProfile::Disabled => "full access",
        PermissionProfile::External { .. } => "external sandbox",
        profile @ PermissionProfile::Managed { .. } => {
            let fs = profile.file_system_sandbox_policy();
            let cwd = Path::new(&shell.cwd);
            if fs.has_full_disk_write_access() {
                "managed access"
            } else if fs.can_write_local_path_with_cwd(cwd, cwd) {
                "workspace write"
            } else if fs.get_writable_roots_with_cwd(cwd).is_empty() {
                "read only"
            } else {
                "custom"
            }
        }
    }
}

fn workspace_change_lines(
    changes: &super::workspace::WorkspaceChangeSummary,
) -> Vec<Line<'static>> {
    [
        ("staged", changes.staged),
        ("unstaged", changes.unstaged),
        ("conflicted", changes.conflicted),
        ("untracked", changes.untracked),
    ]
    .into_iter()
    .filter(|(_label, count)| *count > 0)
    .map(|(label, count)| Line::from(format!("  {label} {}", format_usize(count)).dim()))
    .collect()
}
