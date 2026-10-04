use super::*;
use crate::app_shell::transcript_render::TranscriptRenderCache;
use crate::terminal_hyperlinks::HyperlinkLine;
use pretty_assertions::assert_eq;

fn lines(shell: &ShellState, cache: &mut TranscriptRenderCache) -> Vec<HyperlinkLine> {
    let layout = cache.layout(shell, /*width*/ 90, Path::new(&shell.cwd));
    layout.visible_hyperlink_lines(/*from*/ 0, layout.total_lines)
}

#[test]
fn previews_refresh_when_available_and_drop_links_after_permission_or_host_changes() {
    let home = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    let mut shell = ShellState::snapshot_fixture();
    shell.thread_id = ThreadId::new();
    shell.codex_home = home.path().canonicalize().unwrap();
    shell.cwd = workspace.path().to_string_lossy().into_owned();
    shell.permission_profile = PermissionProfile::read_only();
    shell.runtime_workspace_roots.clear();
    shell
        .resume_cwd_runtime
        .uses_remote_workspace_or_environment = false;
    shell.transcript.clear();
    shell.clear_streaming_assistant();
    shell.push_assistant("::codex-inline-vis{file=\"chart.html\"}");
    let mut cache = TranscriptRenderCache::default();
    let missing = lines(&shell, &mut cache);
    assert!(missing.iter().all(|line| line.hyperlinks.is_empty()));
    let thread_id = uuid::Uuid::parse_str(&shell.thread_id.to_string()).unwrap();
    let (seconds, nanos) = thread_id.get_timestamp().unwrap().to_unix();
    let date = chrono::DateTime::from_timestamp(i64::try_from(seconds).unwrap(), nanos).unwrap();
    let directory = shell
        .codex_home
        .join("visualizations")
        .join(date.format("%Y/%m/%d").to_string())
        .join(thread_id.to_string());
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("chart.html"), "<div>chart</div>").unwrap();
    let ready = lines(&shell, &mut cache);
    assert!(ready.iter().flat_map(|line| &line.hyperlinks).any(|link| {
        link.terminal_destination()
            .is_some_and(|url| url.starts_with("file:"))
    }));
    shell.permission_profile = PermissionProfile::Disabled;
    let untrusted = lines(&shell, &mut cache);
    assert_eq!(untrusted, missing);
    shell.permission_profile = PermissionProfile::read_only();
    shell
        .resume_cwd_runtime
        .uses_remote_workspace_or_environment = true;
    assert_eq!(lines(&shell, &mut cache), missing);
}
