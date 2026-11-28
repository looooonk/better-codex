//! Read canonical events for a saved thread whose app-server index is still empty.
use codex_app_server_protocol::Thread;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::Turn;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::project_rollout_line;
use codex_protocol::protocol::EventMsg;
use codex_rollout::RolloutItem;
use std::collections::HashMap;

pub(in crate::app_shell) async fn fill_unindexed_turns(
    thread: &mut Thread,
) -> color_eyre::Result<()> {
    let Some(path) = &thread.path else {
        return Ok(());
    };
    if thread.history_mode != ThreadHistoryMode::Paginated
        || thread.turns.iter().any(|turn| !turn.items.is_empty())
    {
        return Ok(());
    }
    let mut reader = codex_rollout::open_rollout_line_reader(path).await?;
    let mut turns = Vec::<Turn>::new();
    let mut items = HashMap::<(String, String), usize>::new();
    let mut turn_indices = HashMap::<String, usize>::new();
    let mut boundary = 0;
    while let Some(raw) = reader.next_line().await? {
        let Ok(tag) = serde_json::from_str::<super::records::RecordTag>(&raw) else {
            continue;
        };
        if !matches!(tag.kind, "session_meta" | "event_msg") {
            continue;
        }
        let Ok(line) = codex_rollout::parse_rollout_line(&raw) else {
            continue;
        };
        if let RolloutItem::SessionMeta(meta) = &line.item {
            color_eyre::eyre::ensure!(
                meta.meta.id.to_string() == thread.id,
                "Mismatched thread history"
            );
            boundary = meta.meta.subagent_history_start_ordinal.unwrap_or(0);
            continue;
        }
        if line.ordinal.is_some_and(|n| n < boundary) {
            continue;
        }
        if let RolloutItem::EventMsg(EventMsg::ItemCompleted(item)) = &line.item
            && item.thread_id.to_string() != thread.id
        {
            continue;
        }
        let changes = project_rollout_line(&line);
        for meta in changes.changed_turns {
            let mut turn = Turn::from(meta);
            turn.items_view = TurnItemsView::Full;
            if let Some(&i) = turn_indices.get(&turn.id) {
                turn.items = std::mem::take(&mut turns[i].items);
                turns[i] = turn;
            } else {
                turn_indices.insert(turn.id.clone(), turns.len());
                turns.push(turn);
            }
        }
        for change in changes.changed_items {
            let Some(&i) = turn_indices.get(&change.turn_id) else {
                continue;
            };
            let list: &mut Vec<ThreadItem> = &mut turns[i].items;
            match items.entry((change.turn_id, change.item.id().to_owned())) {
                std::collections::hash_map::Entry::Occupied(entry) => {
                    list[*entry.get()] = change.item
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(list.len());
                    list.push(change.item);
                }
            }
        }
    }
    if !turns.is_empty() {
        thread.turns = turns;
    }
    Ok(())
}
