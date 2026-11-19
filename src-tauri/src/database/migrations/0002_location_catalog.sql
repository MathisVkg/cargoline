-- Cargo Line — migration 0002: local location catalog.
--
-- A reusable list of location names for the Add Contract location pickers.
-- cargo_stops keep their own stored location_name; the catalog is a
-- selection source only (no foreign key from stops).
--
-- normalized_name is the case- and whitespace-insensitive identity
-- (trimmed, inner whitespace collapsed, lowercased), so "Port  Tressler"
-- and "port tressler" are the same location.
--
-- After this SQL runs, a Rust hook in the same transaction fills the catalog
-- from the distinct names already stored in cargo_stops.

CREATE TABLE locations (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name    TEXT    NOT NULL CHECK (length(trim(display_name)) > 0),
    normalized_name TEXT    NOT NULL UNIQUE CHECK (length(normalized_name) > 0),
    created_at      INTEGER NOT NULL
) STRICT;
