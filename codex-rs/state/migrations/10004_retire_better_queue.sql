CREATE TRIGGER better_queue_retired_insert BEFORE INSERT ON thread_queue_items
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;

CREATE TRIGGER better_queue_retired_update BEFORE UPDATE ON thread_queue_items
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;

CREATE TRIGGER better_queue_retired_delete BEFORE DELETE ON thread_queue_items
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;

CREATE TRIGGER better_queue_controls_retired_insert BEFORE INSERT ON thread_queue_controls
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;

CREATE TRIGGER better_queue_controls_retired_update BEFORE UPDATE ON thread_queue_controls
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;

CREATE TRIGGER better_queue_controls_retired_delete BEFORE DELETE ON thread_queue_controls
BEGIN
    SELECT RAISE(ABORT, 'queue storage upgraded; restart Better Codex');
END;
