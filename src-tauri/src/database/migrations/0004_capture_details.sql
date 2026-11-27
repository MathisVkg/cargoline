-- 0004: details of a stored screenshot.
-- capture_method (existing) stays the *trigger*: 'manual' (user opened Add Contract
-- or pressed Capture) or 'auto_capture' (future Game.log trigger).
-- New columns are nullable so rows written before this version stay valid.
ALTER TABLE contract_captures ADD COLUMN capture_source TEXT
    CHECK (capture_source IS NULL OR capture_source IN ('window', 'display'));
ALTER TABLE contract_captures ADD COLUMN capture_backend TEXT
    CHECK (capture_backend IS NULL OR capture_backend IN ('wgc', 'print_window'));
ALTER TABLE contract_captures ADD COLUMN width INTEGER CHECK (width IS NULL OR width > 0);
ALTER TABLE contract_captures ADD COLUMN height INTEGER CHECK (height IS NULL OR height > 0);
