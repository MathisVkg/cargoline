-- 0005: OCR processing states for captures (Task 08): pending -> processing -> done | failed.
-- The 0001 CHECK allowed 'recognized' / 'partial' but not 'done'. SQLite cannot change a CHECK
-- constraint in place, so the table is rebuilt; rows are copied ('recognized' / 'partial' -> 'done').
-- Nothing references contract_captures, so dropping and renaming is safe with foreign keys on.
CREATE TABLE contract_captures_new (
    id                INTEGER PRIMARY KEY,
    contract_id       INTEGER REFERENCES contracts (id) ON DELETE SET NULL,
    file_path         TEXT    NOT NULL UNIQUE CHECK (length(trim(file_path)) > 0),
    captured_at       INTEGER NOT NULL,
    expires_at        INTEGER NOT NULL,
    capture_method    TEXT    NOT NULL CHECK (capture_method IN ('manual', 'auto_capture')),
    processing_status TEXT    NOT NULL DEFAULT 'pending'
                              CHECK (processing_status IN ('pending', 'processing', 'done', 'failed')),
    capture_source    TEXT    CHECK (capture_source IS NULL OR capture_source IN ('window', 'display')),
    capture_backend   TEXT    CHECK (capture_backend IS NULL OR capture_backend IN ('wgc', 'print_window')),
    width             INTEGER CHECK (width IS NULL OR width > 0),
    height            INTEGER CHECK (height IS NULL OR height > 0),
    CHECK (expires_at > captured_at AND expires_at <= captured_at + 604800000)
) STRICT;

INSERT INTO contract_captures_new (id, contract_id, file_path, captured_at, expires_at, capture_method,
                                   processing_status, capture_source, capture_backend, width, height)
SELECT id, contract_id, file_path, captured_at, expires_at, capture_method,
       CASE processing_status WHEN 'recognized' THEN 'done' WHEN 'partial' THEN 'done' ELSE processing_status END,
       capture_source, capture_backend, width, height
FROM contract_captures;

DROP TABLE contract_captures;
ALTER TABLE contract_captures_new RENAME TO contract_captures;

CREATE INDEX contract_captures_contract_id_idx ON contract_captures (contract_id);
CREATE INDEX contract_captures_expires_at_idx ON contract_captures (expires_at);
