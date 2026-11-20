-- Cargo Line — migration 0006: run numbers are never reused (Task 12).
--
-- Run numbers used to be MAX(run_number) + 1. Clear History deletes runs, so the next run
-- could get a number that was already shown. run_sequence keeps the highest number ever
-- assigned (a forward-only high-water mark), seeded from the existing runs.

CREATE TABLE run_sequence (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    last_run_number INTEGER NOT NULL CHECK (last_run_number >= 0)
) STRICT;

INSERT INTO run_sequence (id, last_run_number)
SELECT 1, COALESCE(MAX(run_number), 0) FROM runs;

-- The mark only moves forward.
CREATE TRIGGER run_sequence_forward_only
BEFORE UPDATE OF last_run_number ON run_sequence
WHEN NEW.last_run_number < OLD.last_run_number
BEGIN
    SELECT RAISE(ABORT, 'last_run_number cannot decrease');
END;

CREATE TRIGGER run_sequence_no_delete
BEFORE DELETE ON run_sequence
BEGIN
    SELECT RAISE(ABORT, 'run_sequence cannot be deleted');
END;

-- A new run must be above every number ever assigned, not only the runs still present.
DROP TRIGGER runs_number_increasing;
CREATE TRIGGER runs_number_increasing
BEFORE INSERT ON runs
WHEN NEW.run_number <= MAX(
    (SELECT COALESCE(MAX(run_number), 0) FROM runs),
    (SELECT last_run_number FROM run_sequence WHERE id = 1)
)
BEGIN
    SELECT RAISE(ABORT, 'run_number must be greater than every run_number ever assigned');
END;

CREATE TRIGGER runs_advance_sequence
AFTER INSERT ON runs
BEGIN
    UPDATE run_sequence SET last_run_number = NEW.run_number
    WHERE id = 1 AND last_run_number < NEW.run_number;
END;
