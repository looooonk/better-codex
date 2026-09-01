ALTER TABLE thread_turns ADD COLUMN abort_reason TEXT CHECK (
    abort_reason IS NULL OR abort_reason IN (
        'interrupted',
        'replaced',
        'review_ended',
        'budget_limited'
    )
);

DELETE FROM thread_history_projection_state
WHERE thread_id IN (
    SELECT DISTINCT thread_id
    FROM thread_turns
    WHERE status = 'interrupted'
);

CREATE TRIGGER thread_history_missing_abort_reason_turn_insert
AFTER INSERT ON thread_turns
WHEN NEW.status = 'interrupted' AND NEW.abort_reason IS NULL
BEGIN
    DELETE FROM thread_history_projection_state WHERE thread_id = NEW.thread_id;
END;

CREATE TRIGGER thread_history_missing_abort_reason_turn_update
AFTER UPDATE OF status, abort_reason ON thread_turns
WHEN NEW.status = 'interrupted' AND NEW.abort_reason IS NULL
BEGIN
    DELETE FROM thread_history_projection_state WHERE thread_id = NEW.thread_id;
END;

CREATE TRIGGER thread_history_missing_abort_reason_projection_insert
AFTER INSERT ON thread_history_projection_state
WHEN EXISTS (
    SELECT 1
    FROM thread_turns
    WHERE thread_id = NEW.thread_id
      AND status = 'interrupted'
      AND abort_reason IS NULL
)
BEGIN
    DELETE FROM thread_history_projection_state WHERE thread_id = NEW.thread_id;
END;

CREATE TRIGGER thread_history_missing_abort_reason_projection_update
AFTER UPDATE ON thread_history_projection_state
WHEN EXISTS (
    SELECT 1
    FROM thread_turns
    WHERE thread_id = NEW.thread_id
      AND status = 'interrupted'
      AND abort_reason IS NULL
)
BEGIN
    DELETE FROM thread_history_projection_state WHERE thread_id = NEW.thread_id;
END;
