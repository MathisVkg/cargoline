//! Persistent application settings stored in the `app_settings` table.
//!
//! Each field of [`AppSettings`] is one row: the key is the field name and the
//! value is its JSON encoding. Values are validated before every write and
//! after every read; invalid stored data is reported, never silently replaced.

pub mod models;

use rusqlite::{params, Connection};
use serde_json::{Map, Value};

use crate::database::now_utc_ms;
pub use models::{AppSettings, CaptureSource, ContractGrouping, InvalidSetting};

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error(transparent)]
    Invalid(#[from] InvalidSetting),
    #[error("stored settings are invalid ({0}); they were left unchanged")]
    InvalidStored(String),
    #[error("settings could not be encoded: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("settings database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// Loads settings, using defaults for any field that has never been saved.
pub fn load(conn: &Connection) -> Result<AppSettings, SettingsError> {
    let mut merged = to_object(&AppSettings::default())?;

    let mut stmt = conn.prepare_cached("SELECT key, value FROM app_settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (key, raw) = row?;
        // Keys from other (older or newer) versions are ignored, not deleted.
        if let Some(slot) = merged.get_mut(&key) {
            *slot = serde_json::from_str(&raw)
                .map_err(|e| SettingsError::InvalidStored(format!("`{key}`: {e}")))?;
        }
    }

    let settings: AppSettings = serde_json::from_value(Value::Object(merged))
        .map_err(|e| SettingsError::InvalidStored(e.to_string()))?;
    settings
        .validate()
        .map_err(|e| SettingsError::InvalidStored(e.to_string()))?;
    Ok(settings)
}

/// Validates and persists every field atomically.
pub fn save(conn: &mut Connection, settings: &AppSettings) -> Result<(), SettingsError> {
    settings.validate()?;
    let tx = conn.transaction()?;
    write_all(&tx, settings)?;
    tx.commit()?;
    Ok(())
}

/// Loads, modifies and saves settings in one transaction.
/// Nothing is written if `change` produces invalid settings.
pub fn update(
    conn: &mut Connection,
    change: impl FnOnce(&mut AppSettings),
) -> Result<AppSettings, SettingsError> {
    let tx = conn.transaction()?;
    let mut settings = load(&tx)?;
    change(&mut settings);
    settings.validate()?;
    write_all(&tx, &settings)?;
    tx.commit()?;
    Ok(settings)
}

/// Removes every stored setting so all fields return to their defaults.
pub fn restore_defaults(conn: &mut Connection) -> Result<AppSettings, SettingsError> {
    let tx = conn.transaction()?;
    tx.execute("DELETE FROM app_settings", [])?;
    tx.commit()?;
    Ok(AppSettings::default())
}

fn write_all(conn: &Connection, settings: &AppSettings) -> Result<(), SettingsError> {
    let now = now_utc_ms();
    let mut stmt = conn.prepare_cached(
        "INSERT INTO app_settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at
         WHERE value IS NOT excluded.value",
    )?;
    for (key, value) in to_object(settings)? {
        stmt.execute(params![key, serde_json::to_string(&value)?, now])?;
    }
    Ok(())
}

fn to_object(settings: &AppSettings) -> Result<Map<String, Value>, SettingsError> {
    match serde_json::to_value(settings)? {
        Value::Object(map) => Ok(map),
        _ => unreachable!("AppSettings always serializes to a JSON object"),
    }
}
