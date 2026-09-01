CREATE TABLE thread_queue_items (
    id TEXT PRIMARY KEY NOT NULL,
    thread_id TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    payload_digest TEXT NOT NULL,
    client_user_message_id TEXT NOT NULL CHECK (
        length(CAST(client_user_message_id AS BLOB)) BETWEEN 1 AND 256
    ),
    queue_order INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'starting', 'inflight', 'terminal')),
    turn_id TEXT,
    terminal_status TEXT CHECK (
        terminal_status IS NULL OR terminal_status IN ('completed', 'failed', 'interrupted')
    ),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK (
        (state = 'pending' AND turn_id IS NULL AND terminal_status IS NULL)
        OR (state IN ('starting', 'inflight') AND turn_id IS NOT NULL AND terminal_status IS NULL)
        OR (state = 'terminal' AND turn_id IS NOT NULL AND terminal_status IS NOT NULL)
    )
);

CREATE UNIQUE INDEX thread_queue_pending_order_idx
    ON thread_queue_items(thread_id, queue_order)
    WHERE state != 'terminal';

CREATE UNIQUE INDEX thread_queue_active_idx
    ON thread_queue_items(thread_id)
    WHERE state IN ('starting', 'inflight');

CREATE UNIQUE INDEX thread_queue_turn_idx
    ON thread_queue_items(thread_id, turn_id)
    WHERE turn_id IS NOT NULL;

CREATE UNIQUE INDEX thread_queue_client_message_idx
    ON thread_queue_items(thread_id, client_user_message_id);

CREATE INDEX thread_queue_state_idx
    ON thread_queue_items(thread_id, state, queue_order);
