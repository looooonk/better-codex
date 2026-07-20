use std::path::Path;

use crate::workspace_command::WorkspaceCommand;
use crate::workspace_command::WorkspaceCommandExecutor;

const GIT_ROOT_PREFIX: &str = "@@better-codex-git-root ";
const GIT_ROOT_DEPTH_PREFIX: &str = "@@better-codex-git-root-depth ";
const GIT_COUNTS_PREFIX: &str = "@@better-codex-git-counts ";
const GIT_NOT_REPOSITORY_MARKER: &str = "@@better-codex-not-a-git-repository";
pub(super) const GIT_STATUS_ARG0: &str = "better-codex-git-status";
pub(super) const GIT_STATUS_SCRIPT: &str = r###"git_probe=$(LC_ALL=C git rev-parse --is-inside-work-tree 2>&1)
git_probe_exit=$?
if [ "$git_probe_exit" -ne 0 ]; then
    case "$git_probe" in
        "fatal: not a git repository"*)
            printf '@@better-codex-not-a-git-repository\n'
            exit 0
            ;;
        *)
            printf '%s\n' "$git_probe" >&2
            exit "$git_probe_exit"
            ;;
    esac
fi
case "$git_probe" in
    true)
        ;;
    false)
        printf '@@better-codex-not-a-git-repository\n'
        exit 0
        ;;
    *)
        printf '%s\n' "$git_probe" >&2
        exit 1
        ;;
esac
if [ "$#" -eq 0 ]; then
    set -- .
fi
initial_toplevel=$(LC_ALL=C git -C "$1" rev-parse --show-toplevel) || exit $?
root_depth=0
root_found=0
for candidate in "$@"; do
    candidate_probe=$(LC_ALL=C git -C "$candidate" rev-parse --is-inside-work-tree 2>&1)
    candidate_probe_exit=$?
    if [ "$candidate_probe_exit" -ne 0 ]; then
        case "$candidate_probe" in
            "fatal: not a git repository"*)
                root_depth=$((root_depth + 1))
                continue
                ;;
            *)
                printf '%s\n' "$candidate_probe" >&2
                exit "$candidate_probe_exit"
                ;;
        esac
    fi
    case "$candidate_probe" in
        true)
            candidate_prefix=$(LC_ALL=C git -C "$candidate" rev-parse --show-prefix) || exit $?
            if [ -z "$candidate_prefix" ]; then
                candidate_toplevel=$(LC_ALL=C git -C "$candidate" rev-parse --show-toplevel) || exit $?
                if [ "$candidate_toplevel" = "$initial_toplevel" ]; then
                    root_found=1
                    break
                fi
            fi
            ;;
        false)
            ;;
        *)
            printf '%s\n' "$candidate_probe" >&2
            exit 1
            ;;
    esac
    root_depth=$((root_depth + 1))
done
if [ "$root_found" -ne 1 ]; then
    root_depth=0
fi
printf '@@better-codex-git-root-depth %s\n' "$root_depth"
{
    git status --porcelain=v1 --branch --untracked-files=all
    printf '@@better-codex-git-exit %s\n' "$?"
} |
awk '
BEGIN {
    exit_prefix = "@@better-codex-git-exit "
}
index($0, exit_prefix) == 1 {
    git_status = substr($0, length(exit_prefix) + 1)
    saw_git_status = 1
    next
}
substr($0, 1, 3) == "## " {
    print
    next
}
{
    code = substr($0, 1, 2)
    if (code == "??") {
        untracked++
    } else if (code == "DD" || code == "AU" || code == "UD" || code == "UA" || code == "DU" || code == "AA" || code == "UU") {
        conflicted++
    } else if (index(code, "R") || index(code, "C")) {
        renamed++
    } else if (index(code, "A")) {
        added++
    } else if (index(code, "D")) {
        deleted++
    } else if (index(code, "M") || index(code, "T") || index(code, "m") || index(code, "?")) {
        modified++
    }
}
END {
    if (!saw_git_status || git_status != 0) {
        exit 1
    }
    printf "@@better-codex-git-counts %d %d %d %d %d %d\n", added, modified, deleted, renamed, conflicted, untracked
}
'"###;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceGitStatus {
    pub(super) git_root: Option<std::path::PathBuf>,
    pub(super) branch: Option<String>,
    pub(super) changes: WorkspaceChangeSummary,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum WorkspaceGitStatusProbe {
    Found(WorkspaceGitStatus),
    NotRepository,
    Unavailable,
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
) -> WorkspaceGitStatusProbe {
    let Some(candidate_roots) = cwd
        .ancestors()
        .map(|candidate| candidate.to_str().map(str::to_string))
        .collect::<Option<Vec<_>>>()
    else {
        return WorkspaceGitStatusProbe::Unavailable;
    };
    let Ok(output) = runner
        .run(
            WorkspaceCommand::new(
                [
                    "sh".to_string(),
                    "-c".to_string(),
                    GIT_STATUS_SCRIPT.to_string(),
                    GIT_STATUS_ARG0.to_string(),
                ]
                .into_iter()
                .chain(candidate_roots),
            )
            .cwd(cwd),
        )
        .await
    else {
        return WorkspaceGitStatusProbe::Unavailable;
    };
    if !output.success() {
        return WorkspaceGitStatusProbe::Unavailable;
    }
    parse_git_status_probe(&output.stdout, cwd)
}

fn parse_git_status_probe(stdout: &str, cwd: &Path) -> WorkspaceGitStatusProbe {
    if stdout
        .lines()
        .eq(std::iter::once(GIT_NOT_REPOSITORY_MARKER))
    {
        return WorkspaceGitStatusProbe::NotRepository;
    }
    parse_bounded_git_status(stdout, cwd)
        .map(WorkspaceGitStatusProbe::Found)
        .unwrap_or(WorkspaceGitStatusProbe::Unavailable)
}

fn parse_bounded_git_status(stdout: &str, cwd: &Path) -> Option<WorkspaceGitStatus> {
    let mut compact_counts = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(GIT_COUNTS_PREFIX));
    let changes = parse_compact_counts(compact_counts.next()?)?;
    if compact_counts.next().is_some() {
        return None;
    }
    let mut status = parse_git_status(stdout);
    let mut root_depths = stdout
        .lines()
        .filter_map(|line| line.strip_prefix(GIT_ROOT_DEPTH_PREFIX));
    if let Some(root_depth) = root_depths.next() {
        if root_depths.next().is_some() {
            return None;
        }
        let root_depth = root_depth.parse::<usize>().ok()?;
        status.git_root = cwd
            .ancestors()
            .nth(root_depth)
            .map(std::path::Path::to_path_buf);
    }
    status.git_root.as_ref()?;
    status.changes = changes;
    Some(status)
}

fn parse_git_status(stdout: &str) -> WorkspaceGitStatus {
    let mut status = WorkspaceGitStatus::default();
    let mut compact_changes = None;
    for line in stdout.lines() {
        if let Some(root) = line.strip_prefix(GIT_ROOT_PREFIX) {
            status.git_root = (!root.is_empty()).then(|| std::path::PathBuf::from(root));
            continue;
        }
        if let Some(counts) = line.strip_prefix(GIT_COUNTS_PREFIX) {
            compact_changes = parse_compact_counts(counts);
            continue;
        }
        if let Some(header) = line.strip_prefix("## ") {
            status.branch = parse_branch_header(header);
            continue;
        }
        let Some(code) = line.get(..2) else {
            continue;
        };
        count_status_code(code, &mut status.changes);
    }
    if let Some(compact_changes) = compact_changes {
        status.changes = compact_changes;
    }
    status
}

fn parse_compact_counts(counts: &str) -> Option<WorkspaceChangeSummary> {
    let mut counts = counts.split_whitespace().map(str::parse::<usize>);
    let summary = WorkspaceChangeSummary {
        added: counts.next()?.ok()?,
        modified: counts.next()?.ok()?,
        deleted: counts.next()?.ok()?,
        renamed: counts.next()?.ok()?,
        conflicted: counts.next()?.ok()?,
        untracked: counts.next()?.ok()?,
    };
    counts.next().is_none().then_some(summary)
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
    } else if code.contains('M') || code.contains('T') || code.contains('m') || code.contains('?') {
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
            parse_git_status("@@better-codex-git-root /workspace/project\n## main...origin/main\n"),
            WorkspaceGitStatus {
                git_root: Some(std::path::PathBuf::from("/workspace/project")),
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
 m modified-submodule
 ? untracked-submodule
D  deleted.rs
R  old.rs -> new.rs
UU conflicted.rs
?? new.txt
"
            ),
            WorkspaceGitStatus {
                git_root: None,
                branch: Some("feature/workspace".to_string()),
                changes: WorkspaceChangeSummary {
                    added: 1,
                    modified: 3,
                    deleted: 1,
                    renamed: 1,
                    conflicted: 1,
                    untracked: 1,
                },
            }
        );
    }

    #[test]
    fn compact_counts_override_raw_entries_without_double_counting() {
        let stdout = "\
@@better-codex-git-root /workspace/project
## feature/workspace...origin/feature/workspace
 M partial-entry-before-aggregation.rs
@@better-codex-git-counts 7 11 13 17 19 1000000
";
        let expected = WorkspaceGitStatus {
            git_root: Some(std::path::PathBuf::from("/workspace/project")),
            branch: Some("feature/workspace".to_string()),
            changes: WorkspaceChangeSummary {
                added: 7,
                modified: 11,
                deleted: 13,
                renamed: 17,
                conflicted: 19,
                untracked: 1_000_000,
            },
        };

        assert_eq!(parse_git_status(stdout), expected);
        assert_eq!(
            parse_bounded_git_status(stdout, Path::new("/workspace/project")),
            Some(expected)
        );
    }

    #[test]
    fn root_depth_derives_the_git_root_from_the_lexical_cwd() {
        let stdout = "\
@@better-codex-git-root-depth 2
## main...origin/main
@@better-codex-git-counts 0 0 0 0 0 0
";

        assert_eq!(
            parse_bounded_git_status(stdout, Path::new("/workspace/project/a/b")),
            Some(WorkspaceGitStatus {
                git_root: Some(std::path::PathBuf::from("/workspace/project")),
                branch: Some("main".to_string()),
                changes: WorkspaceChangeSummary::default(),
            })
        );
    }

    #[test]
    fn parses_explicit_probe_outcomes() {
        assert_eq!(
            parse_git_status_probe(GIT_NOT_REPOSITORY_MARKER, Path::new("/workspace/project"),),
            WorkspaceGitStatusProbe::NotRepository
        );
        assert_eq!(
            parse_git_status_probe(
                "@@better-codex-not-a-git-repository\nextra output\n",
                Path::new("/workspace/project"),
            ),
            WorkspaceGitStatusProbe::Unavailable
        );
        assert_eq!(
            parse_git_status_probe(
                "@@better-codex-git-root-depth 0\n@@better-codex-git-counts 0 0 0 0 0 0\n",
                Path::new("/workspace/project"),
            ),
            WorkspaceGitStatusProbe::Found(WorkspaceGitStatus {
                git_root: Some(std::path::PathBuf::from("/workspace/project")),
                ..WorkspaceGitStatus::default()
            })
        );
    }

    #[test]
    fn bounded_status_rejects_missing_or_malformed_counts() {
        assert_eq!(
            parse_bounded_git_status(
                "@@better-codex-git-root /workspace/project\n## main...origin/main\n",
                Path::new("/workspace/project"),
            ),
            None
        );
        assert_eq!(
            parse_bounded_git_status(
                "\
@@better-codex-git-root /workspace/project
## main...origin/main
@@better-codex-git-counts 1 2 3 invalid 5 6
",
                Path::new("/workspace/project"),
            ),
            None
        );
        assert_eq!(
            parse_bounded_git_status(
                "\
## main...origin/main
@@better-codex-git-counts 1 2 3 4 5 6
",
                Path::new("/workspace/project"),
            ),
            None
        );
        assert_eq!(
            parse_bounded_git_status(
                "\
@@better-codex-git-root-depth 1
@@better-codex-git-root-depth 2
@@better-codex-git-counts 1 2 3 4 5 6
",
                Path::new("/workspace/project/a/b"),
            ),
            None
        );
    }

    #[test]
    fn git_status_script_bounds_output_for_many_untracked_files() {
        let repository = tempfile::tempdir().expect("temporary repository should be created");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .status()
            .expect("git init should run");
        assert!(initialized.success());
        for index in 0..300 {
            let name = format!("{index:03}-{}.txt", "x".repeat(/*n*/ 220));
            std::fs::write(repository.path().join(name), "")
                .expect("untracked fixture should be written");
        }

        let output = std::process::Command::new("sh")
            .args(["-c", GIT_STATUS_SCRIPT])
            .current_dir(repository.path())
            .output()
            .expect("bounded git status script should run");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("git status output should be UTF-8");

        assert!(stdout.len() < 4 * 1024);
        assert_eq!(
            parse_bounded_git_status(&stdout, repository.path())
                .expect("bounded status metadata should be complete")
                .changes,
            WorkspaceChangeSummary {
                untracked: 300,
                ..WorkspaceChangeSummary::default()
            }
        );
    }

    #[test]
    fn git_status_script_propagates_git_failures() {
        let repository = tempfile::tempdir().expect("temporary repository should be created");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .status()
            .expect("git init should run");
        assert!(initialized.success());
        std::fs::write(repository.path().join(".git/index"), "invalid index")
            .expect("invalid index fixture should be written");

        let output = std::process::Command::new("sh")
            .args(["-c", GIT_STATUS_SCRIPT])
            .current_dir(repository.path())
            .output()
            .expect("bounded git status script should run");

        assert!(!output.status.success());
    }

    #[test]
    fn git_status_script_reports_a_non_repository_explicitly() {
        let directory = tempfile::tempdir().expect("temporary directory should be created");
        let output = std::process::Command::new("sh")
            .args(["-c", GIT_STATUS_SCRIPT])
            .current_dir(directory.path())
            .output()
            .expect("bounded git status script should run");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("git status output should be UTF-8");

        assert_eq!(
            parse_git_status_probe(&stdout, directory.path()),
            WorkspaceGitStatusProbe::NotRepository
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_status_preserves_a_symlinked_lexical_root_with_control_characters() {
        let sandbox = tempfile::tempdir().expect("temporary sandbox should be created");
        let physical_parent = sandbox.path().join("physical-parent");
        let physical_repository = physical_parent.join("repository");
        let nested = physical_repository.join("nested");
        std::fs::create_dir_all(&nested).expect("nested repository path should be created");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&physical_repository)
            .status()
            .expect("git init should run");
        assert!(initialized.success());

        let lexical_parent = sandbox.path().join("alias\n\tparent");
        std::os::unix::fs::symlink(&physical_parent, &lexical_parent)
            .expect("lexical parent symlink should be created");
        let lexical_repository = lexical_parent.join("repository");
        let lexical_cwd = lexical_repository.join("nested");
        let mut command = std::process::Command::new("sh");
        command
            .args(["-c", GIT_STATUS_SCRIPT, GIT_STATUS_ARG0])
            .args(lexical_cwd.ancestors());
        let output = command
            .current_dir(&lexical_cwd)
            .output()
            .expect("bounded git status script should run");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("git status output should be UTF-8");

        assert_eq!(
            parse_bounded_git_status(&stdout, &lexical_cwd)
                .expect("bounded status metadata should be complete")
                .git_root,
            Some(lexical_repository)
        );
        assert!(!stdout.contains("alias\n\tparent"));
    }

    #[cfg(unix)]
    #[test]
    fn git_status_finds_the_lexical_root_above_an_internal_symlink() {
        let repository = tempfile::tempdir().expect("temporary repository should be created");
        let physical_cwd = repository.path().join("real/sub");
        std::fs::create_dir_all(&physical_cwd).expect("physical cwd should be created");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(repository.path())
            .status()
            .expect("git init should run");
        assert!(initialized.success());

        let lexical_cwd = repository.path().join("link");
        std::os::unix::fs::symlink(&physical_cwd, &lexical_cwd)
            .expect("internal cwd symlink should be created");
        let mut command = std::process::Command::new("sh");
        command
            .args(["-c", GIT_STATUS_SCRIPT, GIT_STATUS_ARG0])
            .args(lexical_cwd.ancestors());
        let output = command
            .current_dir(&lexical_cwd)
            .output()
            .expect("bounded git status script should run");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("git status output should be UTF-8");

        assert_eq!(
            parse_bounded_git_status(&stdout, &lexical_cwd)
                .expect("bounded status metadata should be complete")
                .git_root,
            Some(repository.path().to_path_buf())
        );
    }

    #[cfg(unix)]
    #[test]
    fn git_status_falls_back_to_a_symlinked_cwd_without_a_logical_git_ancestor() {
        let sandbox = tempfile::tempdir().expect("temporary sandbox should be created");
        let repository = sandbox.path().join("repository");
        let physical_cwd = repository.join("sub");
        std::fs::create_dir_all(&physical_cwd).expect("physical cwd should be created");
        let initialized = std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repository)
            .status()
            .expect("git init should run");
        assert!(initialized.success());

        let lexical_cwd = sandbox.path().join("outside-link");
        std::os::unix::fs::symlink(&physical_cwd, &lexical_cwd)
            .expect("outside cwd symlink should be created");
        let mut command = std::process::Command::new("sh");
        command
            .args(["-c", GIT_STATUS_SCRIPT, GIT_STATUS_ARG0])
            .args(lexical_cwd.ancestors());
        let output = command
            .current_dir(&lexical_cwd)
            .output()
            .expect("bounded git status script should run");
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).expect("git status output should be UTF-8");

        assert_eq!(
            parse_bounded_git_status(&stdout, &lexical_cwd)
                .expect("bounded status metadata should be complete")
                .git_root,
            Some(lexical_cwd)
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
