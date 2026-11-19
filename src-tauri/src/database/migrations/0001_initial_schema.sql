-- Cargo Line — migration 0001: initial schema.
--
-- Conventions
--   * Timestamps are INTEGER milliseconds since the Unix epoch, UTC.
--   * Local calendar dates are TEXT 'YYYY-MM-DD'.
--   * SCU quantities are INTEGER thousandths of an SCU ("milli-SCU").
--   * Enumerated values are lowercase TEXT guarded by CHECK constraints.
--   * Tables are STRICT so SQLite enforces declared column types.
--   * Derived totals (e.g. active SCU) are never stored.
--
-- This file is applied once, inside a transaction, by rusqlite_migration.
-- Never edit an applied migration; add a new numbered file instead.

-- Mark the file as a Cargo Line database (0x43474C4E, "CGLN"). Written in the
-- same transaction as the schema, so it is only present on a complete schema.
PRAGMA application_id = 1128746062;

------------------------------------------------------------------------------
-- runs: automatic date-based contract groups (one per local calendar date)
------------------------------------------------------------------------------
CREATE TABLE runs (
    id                 INTEGER PRIMARY KEY,
    run_number         INTEGER NOT NULL UNIQUE CHECK (run_number > 0),
    run_date           TEXT    NOT NULL UNIQUE
                               CHECK (run_date IS date(run_date)),
    created_at         INTEGER NOT NULL,
    -- Real-world offsets range from UTC-12:00 to UTC+14:00.
    utc_offset_minutes INTEGER NOT NULL
                               CHECK (utc_offset_minutes BETWEEN -720 AND 840)
) STRICT;

-- Run numbers must strictly increase with every new run.
CREATE TRIGGER runs_number_increasing
BEFORE INSERT ON runs
WHEN NEW.run_number <= (SELECT COALESCE(MAX(run_number), 0) FROM runs)
BEGIN
    SELECT RAISE(ABORT, 'run_number must be greater than every existing run_number');
END;

-- A run's identity (number and date) is permanent.
CREATE TRIGGER runs_identity_immutable
BEFORE UPDATE OF run_number, run_date ON runs
WHEN NEW.run_number IS NOT OLD.run_number OR NEW.run_date IS NOT OLD.run_date
BEGIN
    SELECT RAISE(ABORT, 'run_number and run_date cannot be changed');
END;

------------------------------------------------------------------------------
-- contracts
------------------------------------------------------------------------------
CREATE TABLE contracts (
    -- AUTOINCREMENT: identifiers are never reused, even after deletion.
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id          INTEGER NOT NULL REFERENCES runs (id) ON DELETE RESTRICT,
    title           TEXT    NOT NULL CHECK (length(trim(title)) > 0),
    reward_auec     INTEGER NOT NULL CHECK (reward_auec >= 0),
    status          TEXT    NOT NULL DEFAULT 'active'
                            CHECK (status IN ('active', 'completed', 'abandoned')),
    accepted_at     INTEGER NOT NULL,
    finished_at     INTEGER,
    source          TEXT    NOT NULL
                            CHECK (source IN ('manual', 'ocr', 'auto_capture')),
    game_mission_id TEXT    CHECK (game_mission_id IS NULL OR length(trim(game_mission_id)) > 0),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    -- Active contracts have no finish time; finished ones always do.
    CHECK ((status = 'active') = (finished_at IS NULL)),
    CHECK (finished_at IS NULL OR finished_at >= accepted_at),
    CHECK (updated_at >= created_at)
) STRICT;

CREATE INDEX contracts_run_id_idx ON contracts (run_id);
CREATE INDEX contracts_status_idx ON contracts (status);
CREATE INDEX contracts_game_mission_id_idx ON contracts (game_mission_id)
    WHERE game_mission_id IS NOT NULL;

-- A contract permanently belongs to the run of its acceptance date.
CREATE TRIGGER contracts_run_immutable
BEFORE UPDATE OF run_id ON contracts
WHEN NEW.run_id IS NOT OLD.run_id
BEGIN
    SELECT RAISE(ABORT, 'a contract cannot be moved to another run');
END;

-- Protect active contracts from accidental loss: only completed or abandoned
-- (archived) contracts may be deleted, e.g. by a future Clear History.
CREATE TRIGGER contracts_protect_active_delete
BEFORE DELETE ON contracts
WHEN OLD.status = 'active'
BEGIN
    SELECT RAISE(ABORT, 'active contracts cannot be deleted');
END;

------------------------------------------------------------------------------
-- cargo_requirements: distinct cargo quantities of a contract
-- (SCU totals are computed from these rows only, never from stops)
------------------------------------------------------------------------------
CREATE TABLE cargo_requirements (
    id                     INTEGER PRIMARY KEY,
    contract_id            INTEGER NOT NULL REFERENCES contracts (id) ON DELETE CASCADE,
    commodity_name         TEXT    NOT NULL CHECK (length(trim(commodity_name)) > 0),
    quantity_scu_milli     INTEGER NOT NULL CHECK (quantity_scu_milli > 0),
    max_box_size_scu_milli INTEGER CHECK (max_box_size_scu_milli IS NULL OR max_box_size_scu_milli > 0),
    created_at             INTEGER NOT NULL
) STRICT;

CREATE INDEX cargo_requirements_contract_id_idx ON cargo_requirements (contract_id);

------------------------------------------------------------------------------
-- cargo_stops: pickup / delivery locations of a requirement
-- quantity_scu_milli is an optional allocation; NULL means "unknown".
------------------------------------------------------------------------------
CREATE TABLE cargo_stops (
    id                 INTEGER PRIMARY KEY,
    requirement_id     INTEGER NOT NULL REFERENCES cargo_requirements (id) ON DELETE CASCADE,
    operation          TEXT    NOT NULL CHECK (operation IN ('pickup', 'delivery')),
    location_name      TEXT    NOT NULL CHECK (length(trim(location_name)) > 0),
    quantity_scu_milli INTEGER CHECK (quantity_scu_milli IS NULL OR quantity_scu_milli > 0),
    sort_order         INTEGER NOT NULL CHECK (sort_order >= 0),
    UNIQUE (requirement_id, operation, sort_order)
) STRICT;

CREATE INDEX cargo_stops_location_idx ON cargo_stops (operation, location_name);

-- Known allocations of one operation may never exceed the requirement's
-- quantity, so the same cargo can never be counted twice.
CREATE TRIGGER cargo_stops_allocation_insert
BEFORE INSERT ON cargo_stops
WHEN NEW.quantity_scu_milli IS NOT NULL
 AND NEW.quantity_scu_milli + (
        SELECT COALESCE(SUM(quantity_scu_milli), 0) FROM cargo_stops
        WHERE requirement_id = NEW.requirement_id AND operation = NEW.operation
     ) > (SELECT quantity_scu_milli FROM cargo_requirements WHERE id = NEW.requirement_id)
BEGIN
    SELECT RAISE(ABORT, 'stop allocations exceed the requirement quantity');
END;

CREATE TRIGGER cargo_stops_allocation_update
BEFORE UPDATE OF quantity_scu_milli, operation, requirement_id ON cargo_stops
WHEN NEW.quantity_scu_milli IS NOT NULL
 AND NEW.quantity_scu_milli + (
        SELECT COALESCE(SUM(quantity_scu_milli), 0) FROM cargo_stops
        WHERE requirement_id = NEW.requirement_id AND operation = NEW.operation
          AND id <> OLD.id
     ) > (SELECT quantity_scu_milli FROM cargo_requirements WHERE id = NEW.requirement_id)
BEGIN
    SELECT RAISE(ABORT, 'stop allocations exceed the requirement quantity');
END;

CREATE TRIGGER cargo_requirements_quantity_covers_allocations
BEFORE UPDATE OF quantity_scu_milli ON cargo_requirements
WHEN NEW.quantity_scu_milli < (
        SELECT COALESCE(MAX(total), 0) FROM (
            SELECT SUM(quantity_scu_milli) AS total FROM cargo_stops
            WHERE requirement_id = NEW.id GROUP BY operation
        )
     )
BEGIN
    SELECT RAISE(ABORT, 'requirement quantity is below existing stop allocations');
END;

------------------------------------------------------------------------------
-- cargo_box_groups: box-size breakdown (no visual coordinates stored)
------------------------------------------------------------------------------
CREATE TABLE cargo_box_groups (
    id                 INTEGER PRIMARY KEY,
    requirement_id     INTEGER NOT NULL REFERENCES cargo_requirements (id) ON DELETE CASCADE,
    box_size_scu_milli INTEGER NOT NULL CHECK (box_size_scu_milli > 0),
    box_count          INTEGER NOT NULL CHECK (box_count > 0),
    source_quality     TEXT    NOT NULL CHECK (source_quality IN ('exact', 'estimated')),
    UNIQUE (requirement_id, box_size_scu_milli)
) STRICT;

------------------------------------------------------------------------------
-- contract_captures: metadata of temporary screenshots (files stay on disk)
-- Retention is at most seven days (604 800 000 ms).
------------------------------------------------------------------------------
CREATE TABLE contract_captures (
    id                INTEGER PRIMARY KEY,
    contract_id       INTEGER REFERENCES contracts (id) ON DELETE SET NULL,
    file_path         TEXT    NOT NULL UNIQUE CHECK (length(trim(file_path)) > 0),
    captured_at       INTEGER NOT NULL,
    expires_at        INTEGER NOT NULL,
    capture_method    TEXT    NOT NULL CHECK (capture_method IN ('manual', 'auto_capture')),
    processing_status TEXT    NOT NULL DEFAULT 'pending'
                              CHECK (processing_status IN ('pending', 'processing', 'recognized', 'partial', 'failed')),
    CHECK (expires_at > captured_at AND expires_at <= captured_at + 604800000)
) STRICT;

CREATE INDEX contract_captures_contract_id_idx ON contract_captures (contract_id);
CREATE INDEX contract_captures_expires_at_idx ON contract_captures (expires_at);

------------------------------------------------------------------------------
-- ships: bundled community-sourced catalog (no relation to contracts/runs)
------------------------------------------------------------------------------
CREATE TABLE ships (
    -- Stable catalog identifier (e.g. a slug from the bundled catalog).
    id                       TEXT    PRIMARY KEY CHECK (length(trim(id)) > 0),
    manufacturer             TEXT    NOT NULL CHECK (length(trim(manufacturer)) > 0),
    name                     TEXT    NOT NULL CHECK (length(trim(name)) > 0),
    variant                  TEXT    CHECK (variant IS NULL OR length(trim(variant)) > 0),
    cargo_capacity_scu_milli INTEGER NOT NULL CHECK (cargo_capacity_scu_milli > 0),
    max_box_size_scu_milli   INTEGER CHECK (max_box_size_scu_milli IS NULL OR max_box_size_scu_milli > 0),
    source_url               TEXT,
    verified_at              TEXT    CHECK (verified_at IS NULL OR verified_at IS date(verified_at)),
    availability_status      TEXT
) STRICT;

CREATE INDEX ships_name_idx ON ships (name COLLATE NOCASE);

CREATE TABLE ship_favorites (
    ship_id    TEXT    PRIMARY KEY REFERENCES ships (id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL
) STRICT;

------------------------------------------------------------------------------
-- catalog_metadata: version of the bundled ship catalog (single row)
------------------------------------------------------------------------------
CREATE TABLE catalog_metadata (
    id              INTEGER PRIMARY KEY CHECK (id = 1),
    catalog_version TEXT    NOT NULL CHECK (length(trim(catalog_version)) > 0),
    published_on    TEXT    CHECK (published_on IS NULL OR published_on IS date(published_on)),
    source_summary  TEXT,
    imported_at     INTEGER NOT NULL
) STRICT;

------------------------------------------------------------------------------
-- app_settings: typed settings persisted as validated JSON values
------------------------------------------------------------------------------
CREATE TABLE app_settings (
    key        TEXT    PRIMARY KEY CHECK (length(key) > 0),
    value      TEXT    NOT NULL CHECK (json_valid(value)),
    updated_at INTEGER NOT NULL
) STRICT;
