//! The Activity Log settings (`CORE.STO.SET`, REQ-CORE.JRN.STREAM.ALOG-006).

use super::activity::{ActivityLogConfig, DEFAULT_KEEP, DEFAULT_MAX_BYTES};
use fid_canon::CValue;
use fid_store::settings::Resolved;

/// Rotation size of the Activity Log, in bytes.
pub const MAX_BYTES_KEY: &str = "journal.activity_log_max_bytes";
/// Number of rotated Activity Log files kept.
pub const KEEP_KEY: &str = "journal.activity_log_keep";

fid_store::fid_settings! {
    pub fn activity_settings;
    {
        key: "journal.activity_log_max_bytes",
        owner: "journal",
        schema: r#"{"type": "integer", "minimum": 65536, "maximum": 1099511627776}"#,
        default: "67108864",
        scopes: [Project, User, Machine],
        admin: false,
    }
    {
        key: "journal.activity_log_keep",
        owner: "journal",
        schema: r#"{"type": "integer", "minimum": 0, "maximum": 1000}"#,
        default: "8",
        scopes: [Project, User, Machine],
        admin: false,
    }
}

fn as_u64(v: &CValue) -> Option<u64> {
    match v {
        CValue::U64(n) => Some(*n),
        CValue::I64(n) => u64::try_from(*n).ok(),
        CValue::Num(f) if f.is_finite() && f.fract() == 0.0 && *f >= 0.0 && *f < 9_007_199_254_740_992.0 => {
            // Exact: an integer below 2^53.
            Some(*f as u64)
        }
        _ => None,
    }
}

impl ActivityLogConfig {
    /// The configuration from resolved settings (`fid_store::settings::resolve`). A key that is
    /// missing or not a non-negative integer keeps its default; the resolver has already
    /// checked each value against its schema.
    pub fn from_resolved(values: &[Resolved]) -> ActivityLogConfig {
        let get = |key: &str| values.iter().find(|r| r.key == key).and_then(|r| as_u64(&r.value));
        ActivityLogConfig {
            max_bytes: get(MAX_BYTES_KEY).unwrap_or(DEFAULT_MAX_BYTES),
            keep: get(KEEP_KEY).and_then(|k| usize::try_from(k).ok()).unwrap_or(DEFAULT_KEEP),
        }
    }
}
