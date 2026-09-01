ALTER TABLE thread_queue_controls ADD COLUMN blocked_submission_id TEXT;
ALTER TABLE thread_queue_controls ADD COLUMN blocked_retry_allowed INTEGER CHECK (
    blocked_retry_allowed IS NULL OR blocked_retry_allowed IN (0, 1)
);

CREATE TRIGGER thread_queue_blocked_claim_guard
BEFORE UPDATE OF state ON thread_queue_items
WHEN OLD.state = 'pending'
  AND NEW.state IN ('starting', 'inflight')
  AND EXISTS (
      SELECT 1
      FROM thread_queue_controls AS controls
      WHERE controls.thread_id = OLD.thread_id
        AND controls.blocked_submission_id IS NOT NULL
        AND NOT (
            controls.blocked_submission_id = OLD.id
            AND controls.blocked_retry_allowed = 1
        )
  )
BEGIN
    SELECT RAISE(ABORT, 'blocked queued submission cannot be claimed');
END;

CREATE TRIGGER thread_queue_blocked_payload_guard
BEFORE UPDATE OF payload_json, payload_digest ON thread_queue_items
WHEN EXISTS (
    SELECT 1
    FROM thread_queue_controls AS controls
    WHERE controls.thread_id = OLD.thread_id
      AND controls.blocked_submission_id = OLD.id
      AND controls.blocked_retry_allowed = 0
)
BEGIN
    SELECT RAISE(ABORT, 'blocked queued submission payload is already durable');
END;

CREATE TRIGGER thread_queue_blocked_delete_cleanup
AFTER DELETE ON thread_queue_items
WHEN EXISTS (
    SELECT 1
    FROM thread_queue_controls AS controls
    WHERE controls.thread_id = OLD.thread_id
      AND controls.blocked_submission_id = OLD.id
)
BEGIN
    DELETE FROM thread_queue_controls
    WHERE thread_id = OLD.thread_id
      AND blocked_submission_id = OLD.id;
END;
