//! Typed values for the enumerated columns of the schema.
//!
//! Each enum mirrors a `CHECK (... IN (...))` constraint in the migrations
//! and is the single Rust source of truth for those strings. Row structs and
//! repositories are added together with the features that need them.

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};

macro_rules! text_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $text:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name { $($variant),+ }

        impl $name {
            /// Every allowed value, in declaration order.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// The value stored in SQLite.
            pub const fn as_str(self) -> &'static str {
                match self { $(Self::$variant => $text),+ }
            }

            /// Parses a stored value.
            pub fn parse(value: &str) -> Option<Self> {
                match value { $($text => Some(Self::$variant),)+ _ => None }
            }
        }

        impl ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.as_str()))
            }
        }

        impl FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                let text = value.as_str()?;
                Self::parse(text).ok_or_else(|| {
                    FromSqlError::Other(format!("invalid {} value: {text:?}", stringify!($name)).into())
                })
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }
    };
}

text_enum! {
    /// `contracts.status`
    ContractStatus { Active => "active", Completed => "completed", Abandoned => "abandoned" }
}

text_enum! {
    /// `contracts.source`
    ContractSource { Manual => "manual", Ocr => "ocr", AutoCapture => "auto_capture" }
}

text_enum! {
    /// `cargo_stops.operation`
    CargoOperation { Pickup => "pickup", Delivery => "delivery" }
}

text_enum! {
    /// `cargo_box_groups.source_quality`
    BoxSourceQuality { Exact => "exact", Estimated => "estimated" }
}

text_enum! {
    /// `contract_captures.capture_method`
    CaptureMethod { Manual => "manual", AutoCapture => "auto_capture" }
}

text_enum! {
    /// `contract_captures.processing_status`
    CaptureProcessingStatus {
        Pending => "pending",
        Processing => "processing",
        Recognized => "recognized",
        Partial => "partial",
        Failed => "failed",
    }
}

/// Number of milli-SCU in one SCU (quantities are stored as integer thousandths).
pub const MILLI_SCU_PER_SCU: i64 = 1_000;

/// Maximum screenshot retention: seven days, in milliseconds.
pub const CAPTURE_RETENTION_MS: i64 = 7 * 24 * 60 * 60 * 1_000;
