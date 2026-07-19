use codex_protocol::models::ResponseItem;

pub(crate) fn restore_explicit_user_message_ids(
    compacted_history: &mut [ResponseItem],
    source_history: &[ResponseItem],
) {
    // Compact endpoints can omit transport IDs from echoed input messages. Match the returned
    // messages against source order so genuine user provenance survives that round trip.
    let mut source_start = 0;
    for compacted_item in compacted_history {
        let ResponseItem::Message {
            id, role, content, ..
        } = compacted_item
        else {
            continue;
        };
        let Some((source_offset, source_item)) = source_history[source_start..]
            .iter()
            .enumerate()
            .find(|(_, source_item)| {
                matches!(
                    source_item,
                    ResponseItem::Message {
                        role: source_role,
                        content: source_content,
                        ..
                    } if source_role == role && source_content == content
                )
            })
        else {
            continue;
        };
        source_start = source_start.saturating_add(source_offset).saturating_add(1);
        if crate::context::is_explicit_user_message(source_item) {
            *id = source_item.id().cloned();
        }
    }
}
