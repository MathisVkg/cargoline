-- Cargo Line — migration 0003: reference catalogs (locations + commodities).
--
-- Catalog rows are suggestions only: contracts keep storing plain name
-- strings, with no foreign key to catalog rows. Bundled rows come from the
-- offline community snapshot (synced at startup, not here); user rows come
-- from names the user has typed. Rows are never deleted: bundled rows that
-- leave the snapshot get retired_at instead.

-- 1. Locations: extend the 0002 table in place (keeps one unique name space
--    and the existing reuse logic; ADD COLUMN needs no table rebuild).
ALTER TABLE locations ADD COLUMN source TEXT NOT NULL DEFAULT 'user'
    CHECK (source IN ('bundled', 'user'));
ALTER TABLE locations ADD COLUMN source_id TEXT
    CHECK (source_id IS NULL OR length(source_id) > 0);
ALTER TABLE locations ADD COLUMN kind TEXT
    CHECK (kind IS NULL OR kind IN ('station', 'city', 'outpost', 'distribution_centre', 'other'));
ALTER TABLE locations ADD COLUMN body_name TEXT;
ALTER TABLE locations ADD COLUMN system_name TEXT;
ALTER TABLE locations ADD COLUMN seen_in_hauling INTEGER NOT NULL DEFAULT 0
    CHECK (seen_in_hauling IN (0, 1));
ALTER TABLE locations ADD COLUMN retired_at INTEGER;

CREATE UNIQUE INDEX locations_source_id_idx ON locations (source_id) WHERE source_id IS NOT NULL;

-- 2. Commodities: same pattern. Seeded by a Rust hook from existing
--    cargo_requirements names (user rows, oldest spelling wins).
CREATE TABLE commodities (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name    TEXT    NOT NULL CHECK (length(trim(display_name)) > 0),
    normalized_name TEXT    NOT NULL UNIQUE CHECK (length(normalized_name) > 0),
    source          TEXT    NOT NULL DEFAULT 'user' CHECK (source IN ('bundled', 'user')),
    source_id       TEXT    CHECK (source_id IS NULL OR length(source_id) > 0),
    kind            TEXT,
    seen_in_hauling INTEGER NOT NULL DEFAULT 0 CHECK (seen_in_hauling IN (0, 1)),
    retired_at      INTEGER,
    created_at      INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX commodities_source_id_idx ON commodities (source_id) WHERE source_id IS NOT NULL;

-- 3. catalog_metadata: 0001 allowed a single row (id = 1), which cannot hold
--    both this reference catalog and the future ship catalog. Rebuild it
--    keyed by catalog name. The old table was never written by the app; any
--    row it holds is kept as the 'ships' catalog it was designed for.
CREATE TABLE catalog_metadata_new (
    catalog         TEXT    PRIMARY KEY CHECK (catalog IN ('reference', 'ships')),
    catalog_version TEXT    NOT NULL CHECK (length(trim(catalog_version)) > 0),
    game_version    TEXT,
    source_name     TEXT,
    source_url      TEXT,
    fetched_on      TEXT    CHECK (fetched_on IS NULL OR fetched_on IS date(fetched_on)),
    attribution     TEXT,
    imported_at     INTEGER NOT NULL
) STRICT;

INSERT INTO catalog_metadata_new (catalog, catalog_version, fetched_on, attribution, imported_at)
SELECT 'ships', catalog_version, published_on, source_summary, imported_at FROM catalog_metadata;

DROP TABLE catalog_metadata;
ALTER TABLE catalog_metadata_new RENAME TO catalog_metadata;
