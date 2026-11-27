//! The welcome guide setting: default, round trip, and an older database that has never stored the
//! key (existing users see the guide once after updating). Temporary databases only.

use cargo_line_lib::database::{Database, DATABASE_FILE_NAME};
use cargo_line_lib::settings::{self, AppSettings};

fn db() -> (tempfile::TempDir, Database) {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::open(&dir.path().join(DATABASE_FILE_NAME)).unwrap();
    (dir, db)
}

#[test]
fn the_guide_is_unseen_by_default() {
    assert!(!AppSettings::default().welcome_seen);
    let (_dir, db) = db();
    let conn = db.connection().unwrap();
    assert!(
        !settings::load(&conn).unwrap().welcome_seen,
        "a new data folder"
    );
}

#[test]
fn marking_it_seen_round_trips_and_survives_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(DATABASE_FILE_NAME);
    {
        let db = Database::open(&path).unwrap();
        let mut conn = db.connection().unwrap();
        let saved = settings::update(&mut conn, |s| s.welcome_seen = true).unwrap();
        assert!(saved.welcome_seen);
    }
    // A restart: the same database file, opened again.
    let db = Database::open(&path).unwrap();
    let conn = db.connection().unwrap();
    let loaded = settings::load(&conn).unwrap();
    assert!(loaded.welcome_seen, "it doesn't open again after a restart");
    assert_eq!(
        loaded,
        AppSettings {
            welcome_seen: true,
            ..loaded.clone()
        }
    );
}

#[test]
fn an_older_database_without_the_key_gets_the_default() {
    let (_dir, db) = db();
    let mut conn = db.connection().unwrap();
    // An earlier version stored every field it knew (and a key this version no longer uses).
    settings::update(&mut conn, |s| s.capture_delay_ms = 2_000).unwrap();
    conn.execute("DELETE FROM app_settings WHERE key = 'welcome_seen'", [])
        .unwrap();
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at) VALUES ('automatic_update_checks', 'false', 0)",
        [],
    )
    .unwrap();
    let loaded = settings::load(&conn).unwrap();
    assert!(!loaded.welcome_seen, "shown once after updating");
    assert_eq!(
        loaded.capture_delay_ms, 2_000,
        "the other settings are kept"
    );
}
