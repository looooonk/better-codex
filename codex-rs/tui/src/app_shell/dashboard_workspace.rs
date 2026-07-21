use super::ShellState;
use super::dashboard::dashboard_value;
use super::dashboard::format_usize;
use codex_protocol::models::ManagedFileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use ratatui::style::Stylize;
use ratatui::text::Line;

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
            workspace_lines.push(Line::from("tree clean".green()));
        }
        if let Some(branch) = &git_status.branch {
            workspace_lines.push(Line::from(vec![
                "branch ".dim(),
                dashboard_value(branch, width, /*prefix_width*/ 7).cyan(),
            ]));
        }
    }
    let permission_details = match &shell.permission_profile {
        PermissionProfile::Managed {
            file_system,
            network,
        } => {
            let file_system_label = match file_system {
                ManagedFileSystemPermissions::Restricted { .. } => "restricted",
                ManagedFileSystemPermissions::Unrestricted => "unrestricted",
            };
            workspace_lines.push(profile_line(shell, width, "managed"));
            Some(Line::from(format!(
                "files {file_system_label}, net {network}"
            )))
        }
        PermissionProfile::Disabled => {
            workspace_lines.push(profile_line(shell, width, "full access"));
            None
        }
        PermissionProfile::External { network } => {
            workspace_lines.push(profile_line(shell, width, "external"));
            Some(Line::from(format!("net {network}")))
        }
    };
    workspace_lines.push(Line::from(vec![
        "cwd ".dim(),
        dashboard_value(&shell.cwd, width, /*prefix_width*/ 4).into(),
    ]));
    if let Some(permission_details) = permission_details {
        workspace_lines.push(permission_details);
    }
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

fn profile_line(shell: &ShellState, width: usize, kind: &str) -> Line<'static> {
    let label = shell
        .active_permission_profile
        .as_ref()
        .map(|profile| format!("{} ({kind})", profile.id))
        .unwrap_or_else(|| kind.to_string());
    Line::from(vec![
        "profile ".dim(),
        dashboard_value(&label, width, /*prefix_width*/ 8).into(),
    ])
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
