use super::*;
use anyhow::Context;
use anyhow::Result;
use pretty_assertions::assert_eq;
use std::path::Path;
use tempfile::tempdir;

#[tokio::test]
async fn small_hook_output_remains_inline() -> Result<()> {
    let dir = tempdir()?;
    let output_dir = AbsolutePathBuf::from_absolute_path(dir.path())?.join(HOOK_OUTPUTS_DIR);
    let thread_id = ThreadId::new();
    let spiller = HookOutputSpiller {
        output_dir: output_dir.clone(),
        tracker: HookOutputSpillTracker::default(),
    };

    let output = spiller
        .maybe_spill_text(thread_id, "short".to_string())
        .await;

    assert_eq!(output, "short");
    assert!(!output_dir.exists());
    Ok(())
}

#[tokio::test]
async fn large_hook_output_spills_to_file() -> Result<()> {
    let dir = tempdir()?;
    let text = "hook output ".repeat(1_000);
    let output_dir = AbsolutePathBuf::from_absolute_path(dir.path())?.join(HOOK_OUTPUTS_DIR);
    let spiller = HookOutputSpiller {
        output_dir,
        tracker: HookOutputSpillTracker::default(),
    };

    let output = spiller
        .maybe_spill_text(ThreadId::new(), text.clone())
        .await;

    assert!(output.contains("tokens truncated"));
    let path = output
        .lines()
        .find_map(|line| line.strip_prefix("Full hook output saved to: "))
        .context("spill path")?;
    assert_eq!(fs::read_to_string(path).await?, text);
    Ok(())
}

#[tokio::test]
async fn cleanup_removes_only_owned_hook_outputs() -> Result<()> {
    let dir = tempdir()?;
    let output_dir = AbsolutePathBuf::from_absolute_path(dir.path())?.join(HOOK_OUTPUTS_DIR);
    let first_tracker = HookOutputSpillTracker::default();
    let second_tracker = HookOutputSpillTracker::default();
    let first_spiller = HookOutputSpiller {
        output_dir: output_dir.clone(),
        tracker: first_tracker.clone(),
    };
    let second_spiller = HookOutputSpiller {
        output_dir,
        tracker: second_tracker,
    };
    let text = "hook output ".repeat(1_000);

    let first_output = first_spiller
        .maybe_spill_text(ThreadId::new(), text.clone())
        .await;
    let second_output = second_spiller.maybe_spill_text(ThreadId::new(), text).await;
    let first_path = first_output
        .lines()
        .find_map(|line| line.strip_prefix("Full hook output saved to: "))
        .context("first spill path")?;
    let second_path = second_output
        .lines()
        .find_map(|line| line.strip_prefix("Full hook output saved to: "))
        .context("second spill path")?;

    first_tracker.cleanup().await;

    assert_eq!(
        [
            Path::new(first_path).exists(),
            Path::new(second_path).exists()
        ],
        [false, true]
    );
    Ok(())
}
