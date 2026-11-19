-- Cargo Line — migration 0003: optional Pick Up / Turn In reminders.
--
-- A personal tick per cargo stop: "I've picked this up" / "I've turned this
-- in". One column covers both operations because each stop already has its
-- operation. NULL = not ticked; otherwise the UTC ms when it was ticked.
--
-- Purely informational: it never affects SCU totals, grouping, ordering or
-- whether a contract can be completed.

ALTER TABLE cargo_stops ADD COLUMN reminder_done_at INTEGER
    CHECK (reminder_done_at IS NULL OR reminder_done_at >= 0);
