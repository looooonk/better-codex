use std::path::Path;

use crate::workspace_command::WorkspaceCommand;
use crate::workspace_command::WorkspaceCommandExecutor;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceGitStatus {
    pub(super) branch: Option<String>,
    pub(super) changes: WorkspaceChangeSummary,
}

impl WorkspaceGitStatus {
    pub(super) fn is_dirty(&self) -> bool {
        self.changes.total() > 0
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceChangeSummary {
    pub(super) added: usize,
    pub(super) modified: usize,
    pub(super) deleted: usize,
    pub(super) renamed: usize,
    pub(super) conflicted: usize,
    pub(super) untracked: usize,
}

impl WorkspaceChangeSummary {
    pub(super) fn total(&self) -> usize {
        self.added + self.modified + self.deleted + self.renamed + self.conflicted + self.untracked
    }
}

pub(super) async fn load_git_status(
    runner: &dyn WorkspaceCommandExecutor,
    cwd: &Path,
) -> Option<WorkspaceGitStatus> {
    let output = runner
        .run(WorkspaceCommand::new(["git", "status", "--porcelain=v1", "--branch"]).cwd(cwd))
        .await
        .ok()?;
    output.success().then(|| parse_git_status(&output.stdout))
}

fn parse_git_status(stdout: &str) -> WorkspaceGitStatus {
    let mut status = WorkspaceGitStatus::default();
    for line in stdout.lines() {
        if let Some(header) = line.strip_prefix("## ") {
            status.branch = parse_branch_header(header);
            continue;
        }
        let Some(code) = line.get(..2) else {
            continue;
        };
        count_status_code(code, &mut status.changes);
    }
    status
}

fn parse_branch_header(header: &str) -> Option<String> {
    let branch_candidate = header.strip_prefix("No commits yet on ").unwrap_or(header);
    let branch = branch_candidate
        .split_once("...")
        .map(|(branch, _upstream)| branch)
        .unwrap_or_else(|| {
            branch_candidate
                .split_whitespace()
                .next()
                .unwrap_or_default()
        })
        .trim();
    Some(branch.to_string()).filter(|branch| !branch.is_empty() && branch != "HEAD")
}

fn count_status_code(code: &str, changes: &mut WorkspaceChangeSummary) {
    if code == "??" {
        changes.untracked += 1;
    } else if is_conflicted_status(code) {
        changes.conflicted += 1;
    } else if code.contains('R') || code.contains('C') {
        changes.renamed += 1;
    } else if code.contains('A') {
        changes.added += 1;
    } else if code.contains('D') {
        changes.deleted += 1;
    } else if code.contains('M') || code.contains('T') {
        changes.modified += 1;
    }
}

fn is_conflicted_status(code: &str) -> bool {
    matches!(code, "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn parses_clean_branch_status() {
        assert_eq!(
            parse_git_status("## main...origin/main\n"),
            WorkspaceGitStatus {
                branch: Some("main".to_string()),
                changes: WorkspaceChangeSummary::default(),
            }
        );
    }

    #[test]
    fn parses_dirty_status_by_change_type() {
        assert_eq!(
            parse_git_status(
                "\
## feature/workspace
A  added.rs
 M modified.rs
D  deleted.rs
R  old.rs -> new.rs
UU conflicted.rs
?? new.txt
"
            ),
            WorkspaceGitStatus {
                branch: Some("feature/workspace".to_string()),
                changes: WorkspaceChangeSummary {
                    added: 1,
                    modified: 1,
                    deleted: 1,
                    renamed: 1,
                    conflicted: 1,
                    untracked: 1,
                },
            }
        );
    }

    #[test]
    fn parses_unborn_branch_status() {
        assert_eq!(
            parse_branch_header("No commits yet on main"),
            Some("main".to_string())
        );
    }
}
