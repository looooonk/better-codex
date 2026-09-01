use std::fs;
use std::path::Path;

use codex_protocol::RolloutId;
use codex_protocol::ThreadId;
use codex_protocol::protocol::HistoryPosition;
use codex_protocol::protocol::SessionMeta;
use codex_protocol::protocol::SessionMetaLine;
use codex_protocol::protocol::ThreadHistoryMode;
use codex_rollout::RolloutItem;
use codex_rollout::RolloutLine;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::RolloutLineageSegment;
use super::super::LocalThreadStore;
use super::super::test_support::test_config;

#[tokio::test]
async fn resolves_replacement_lineage_for_one_logical_thread() {
    let home = TempDir::new().expect("temp dir");
    let store = test_store(home.path()).await;
    let thread_id = ThreadId::new();
    let root_id = thread_id;
    let middle_id = RolloutId::new();
    let head_id = RolloutId::new();
    let root_path = write_rollout(
        home.path(),
        thread_id,
        root_id,
        /*history_base*/ None,
        /*next_ordinal*/ 6,
    );
    let root_end = history_position(root_path.as_path(), root_id, /*end_ordinal_exclusive*/ 4);
    let middle_path = write_rollout(
        home.path(),
        thread_id,
        middle_id,
        Some(root_end),
        /*next_ordinal*/ 7,
    );
    let middle_end = history_position(
        middle_path.as_path(),
        middle_id,
        /*end_ordinal_exclusive*/ 6,
    );
    let head_path = write_rollout(
        home.path(),
        thread_id,
        head_id,
        Some(middle_end),
        /*next_ordinal*/ 9,
    );
    seed_selected_rollout(&store, thread_id, head_path.clone()).await;

    let lineage = store
        .resolve_rollout_lineage(thread_id)
        .await
        .expect("resolve replacement lineage");

    assert_eq!(
        lineage.segments,
        vec![
            RolloutLineageSegment {
                rollout_id: root_id,
                rollout_path: root_path,
                start_ordinal: 1,
                end: Some(root_end),
            },
            RolloutLineageSegment {
                rollout_id: middle_id,
                rollout_path: middle_path,
                start_ordinal: 5,
                end: Some(middle_end),
            },
            RolloutLineageSegment {
                rollout_id: head_id,
                rollout_path: head_path,
                start_ordinal: 7,
                end: None,
            },
        ]
    );
}

#[tokio::test]
async fn rejects_missing_cycles_and_out_of_bounds_offsets() {
    let home = TempDir::new().expect("temp dir");
    let store = test_store(home.path()).await;

    let missing_thread = ThreadId::new();
    let missing_head = RolloutId::new();
    let missing_path = write_rollout(
        home.path(),
        missing_thread,
        missing_head,
        Some(unchecked_history_position(RolloutId::new(), 1)),
        /*next_ordinal*/ 2,
    );
    seed_selected_rollout(&store, missing_thread, missing_path).await;
    assert_invalid_lineage(&store, missing_thread, "missing source rollout").await;

    let cycle_thread = ThreadId::new();
    let cycle_a = RolloutId::new();
    let cycle_b = RolloutId::new();
    let cycle_a_path = write_rollout(
        home.path(),
        cycle_thread,
        cycle_a,
        /*history_base*/ None,
        /*next_ordinal*/ 1,
    );
    let cycle_b_path = write_rollout(
        home.path(),
        cycle_thread,
        cycle_b,
        /*history_base*/ None,
        /*next_ordinal*/ 1,
    );
    write_cycle_metadata(
        cycle_a_path.as_path(),
        cycle_a,
        cycle_b_path.as_path(),
        cycle_b,
    );
    seed_selected_rollout(&store, cycle_thread, cycle_a_path).await;
    assert_invalid_lineage(&store, cycle_thread, "cycle detected").await;

    let invalid_thread = ThreadId::new();
    let invalid_root = invalid_thread;
    let invalid_head = RolloutId::new();
    let invalid_root_path = write_rollout(
        home.path(),
        invalid_thread,
        invalid_root,
        /*history_base*/ None,
        /*next_ordinal*/ 2,
    );
    let invalid_path = write_rollout(
        home.path(),
        invalid_thread,
        invalid_head,
        Some(HistoryPosition {
            thread_id: invalid_root,
            end_ordinal_exclusive: 2,
            end_byte_offset: fs::metadata(invalid_root_path)
                .expect("root metadata")
                .len()
                + 1,
        }),
        /*next_ordinal*/ 3,
    );
    seed_selected_rollout(&store, invalid_thread, invalid_path).await;
    assert_invalid_lineage(
        &store,
        invalid_thread,
        "rollout boundary is past the final record",
    )
    .await;
}

#[tokio::test]
async fn rejects_cutoffs_inside_records_and_with_mismatched_ordinals() {
    let home = TempDir::new().expect("temp dir");
    let store = test_store(home.path()).await;

    for (end_ordinal_exclusive, offset_adjustment, detail) in [
        (2, -1i64, "rollout boundary is inside a JSONL record"),
        (
            3,
            0,
            "cutoff byte offset disagrees with rollout ordinals",
        ),
    ] {
        let thread_id = ThreadId::new();
        let root_path = write_rollout(
            home.path(),
            thread_id,
            thread_id,
            /*history_base*/ None,
            /*next_ordinal*/ 4,
        );
        let offset = rollout_end_byte_offset(root_path.as_path(), /*end_ordinal_exclusive*/ 2);
        let end_byte_offset = if offset_adjustment < 0 {
            offset - offset_adjustment.unsigned_abs()
        } else {
            offset + offset_adjustment as u64
        };
        let head_id = RolloutId::new();
        let head_path = write_rollout(
            home.path(),
            thread_id,
            head_id,
            Some(HistoryPosition {
                thread_id,
                end_ordinal_exclusive,
                end_byte_offset,
            }),
            /*next_ordinal*/ 5,
        );
        seed_selected_rollout(&store, thread_id, head_path).await;
        assert_invalid_lineage(&store, thread_id, detail).await;
    }
}

#[tokio::test]
async fn rejects_ancestor_rollouts_owned_by_another_logical_thread() {
    let home = TempDir::new().expect("temp dir");
    let store = test_store(home.path()).await;
    let thread_id = ThreadId::new();
    let other_thread_id = ThreadId::new();
    let ancestor_id = RolloutId::new();
    let ancestor_path = write_rollout(
        home.path(),
        other_thread_id,
        ancestor_id,
        /*history_base*/ None,
        /*next_ordinal*/ 3,
    );
    let head_path = write_rollout(
        home.path(),
        thread_id,
        RolloutId::new(),
        Some(history_position(
            ancestor_path.as_path(),
            ancestor_id,
            /*end_ordinal_exclusive*/ 2,
        )),
        /*next_ordinal*/ 4,
    );
    seed_selected_rollout(&store, thread_id, head_path).await;

    assert_invalid_lineage(&store, thread_id, "source rollout belongs to another thread").await;
}

async fn assert_invalid_lineage(store: &LocalThreadStore, thread_id: ThreadId, detail: &str) {
    let err = store
        .resolve_rollout_lineage(thread_id)
        .await
        .expect_err("lineage should be invalid");
    assert!(err.to_string().contains(detail), "{err}");
}

async fn seed_selected_rollout(
    store: &LocalThreadStore,
    thread_id: ThreadId,
    rollout_path: std::path::PathBuf,
) {
    let runtime = store.state_db().await.expect("state runtime");
    let mut builder = codex_state::ThreadMetadataBuilder::new(
        thread_id,
        rollout_path,
        chrono::Utc::now(),
        codex_protocol::protocol::SessionSource::Exec,
    );
    builder.history_mode = ThreadHistoryMode::Paginated;
    runtime
        .upsert_thread(&builder.build(store.config.default_model_provider_id.as_str()))
        .await
        .expect("seed selected rollout");
}

async fn test_store(home: &Path) -> LocalThreadStore {
    let config = test_config(home);
    let runtime = codex_state::StateRuntime::init(
        config.sqlite_home.clone(),
        config.default_model_provider_id.clone(),
    )
    .await
    .expect("state runtime");
    LocalThreadStore::new(config, Some(runtime))
}

fn write_rollout(
    home: &Path,
    thread_id: ThreadId,
    rollout_id: RolloutId,
    history_base: Option<HistoryPosition>,
    next_ordinal: u64,
) -> std::path::PathBuf {
    let directory = home.join("sessions/2026/07/16");
    fs::create_dir_all(directory.as_path()).expect("create rollout directory");
    let suffix = if rollout_id == thread_id {
        thread_id.to_string()
    } else {
        format!("{thread_id}_{rollout_id}")
    };
    let path = directory.join(format!("rollout-2026-07-16T00-00-00-{suffix}.jsonl"));
    let initial_ordinal = history_base.map_or(0, |base| base.end_ordinal_exclusive);
    let mut lines = vec![rollout_line(
        initial_ordinal,
        RolloutItem::SessionMeta(SessionMetaLine {
            meta: SessionMeta {
                session_id: thread_id.into(),
                id: thread_id,
                history_mode: ThreadHistoryMode::Paginated,
                history_base,
                ..SessionMeta::default()
            },
            git: None,
        }),
    )];
    for ordinal in initial_ordinal + 1..next_ordinal {
        lines.push(rollout_line(
            ordinal,
            RolloutItem::EventMsg(codex_protocol::protocol::EventMsg::ShutdownComplete),
        ));
    }
    fs::write(path.as_path(), format!("{}\n", lines.join("\n"))).expect("write rollout");
    path
}

fn rollout_line(ordinal: u64, item: RolloutItem) -> String {
    serde_json::to_string(&RolloutLine {
        timestamp: "2026-07-16T00:00:00.000Z".to_string(),
        ordinal: Some(ordinal),
        item,
    })
    .expect("serialize rollout line")
}

fn history_position(
    path: &Path,
    rollout_id: RolloutId,
    end_ordinal_exclusive: u64,
) -> HistoryPosition {
    HistoryPosition {
        thread_id: rollout_id,
        end_ordinal_exclusive,
        end_byte_offset: rollout_end_byte_offset(path, end_ordinal_exclusive),
    }
}

fn rollout_end_byte_offset(path: &Path, end_ordinal_exclusive: u64) -> u64 {
    let bytes = fs::read(path).expect("read rollout");
    let byte_count = bytes
        .split_inclusive(|byte| *byte == b'\n')
        .take_while(|line| {
            serde_json::from_slice::<RolloutLine>(line)
                .expect("parse rollout fixture")
                .ordinal
                .expect("paginated rollout ordinal")
                < end_ordinal_exclusive
        })
        .map(<[u8]>::len)
        .sum::<usize>();
    u64::try_from(byte_count).expect("rollout byte offset")
}

fn unchecked_history_position(
    rollout_id: RolloutId,
    end_ordinal_exclusive: u64,
) -> HistoryPosition {
    HistoryPosition {
        thread_id: rollout_id,
        end_ordinal_exclusive,
        end_byte_offset: 0,
    }
}

fn write_cycle_metadata(
    cycle_a_path: &Path,
    cycle_a: RolloutId,
    cycle_b_path: &Path,
    cycle_b: RolloutId,
) {
    let mut cycle_a_offset = 1;
    let mut cycle_b_offset = 1;
    for _ in 0..8 {
        set_history_base_and_ordinal(
            cycle_a_path,
            HistoryPosition {
                thread_id: cycle_b,
                end_ordinal_exclusive: 1,
                end_byte_offset: cycle_b_offset,
            },
            /*ordinal*/ 1,
        );
        set_history_base_and_ordinal(
            cycle_b_path,
            HistoryPosition {
                thread_id: cycle_a,
                end_ordinal_exclusive: 2,
                end_byte_offset: cycle_a_offset,
            },
            /*ordinal*/ 0,
        );
        let next_cycle_a_offset = fs::metadata(cycle_a_path).expect("cycle A metadata").len();
        let next_cycle_b_offset = fs::metadata(cycle_b_path).expect("cycle B metadata").len();
        if (next_cycle_a_offset, next_cycle_b_offset) == (cycle_a_offset, cycle_b_offset) {
            return;
        }
        cycle_a_offset = next_cycle_a_offset;
        cycle_b_offset = next_cycle_b_offset;
    }
    panic!("cycle fixture offsets did not converge");
}

fn set_history_base_and_ordinal(path: &Path, history_base: HistoryPosition, ordinal: u64) {
    let contents = fs::read_to_string(path).expect("read rollout");
    let mut lines = contents.lines();
    let mut head: serde_json::Value =
        serde_json::from_str(lines.next().expect("session metadata")).expect("parse metadata");
    head["ordinal"] = serde_json::json!(ordinal);
    head["payload"]["history_base"] =
        serde_json::to_value(history_base).expect("serialize history base");
    let mut updated = serde_json::to_string(&head).expect("serialize metadata");
    for line in lines {
        updated.push('\n');
        updated.push_str(line);
    }
    updated.push('\n');
    fs::write(path, updated).expect("write history base");
}
