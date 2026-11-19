//! The command bus of Metrology Maven: every surface invokes commands through it
//! (CONSTITUTION C3). Modules land one spec leaf at a time; see `spec/core/bus/`.

pub mod error;
pub mod principal;

/// Short alias for [`error`].
pub use self::error as err;

pub use error::{
    bus_error_schema, bus_errors_digest, bus_errors_json, catalog_value, check_compatible, current_catalog,
    parse_catalog, BusError, Catalog, CatalogError, Code, Detail, Removal, Untrusted, MAX_DETAIL_CHARS, SCHEMA_VERSION,
    TRUNCATION_MARKER,
};
pub use principal::{
    Channel, ChannelKind, ClientId, ConfirmMethod, HumanPresence, IdError, Identity, JournalAppend, OsUser,
    PresenceOutcome, Principal, PrincipalId, PrincipalKind, PrincipalRef, SelfAsserted, SessionId, SessionKey,
    SessionTable, UpgradeClock, UpgradeContext, UpgradeGrant, UpgradeRequest, FORMAT_RANGES, MAX_ID_BYTES,
    MAX_SELF_ASSERTED_CHARS, MAX_UPGRADED_SESSIONS, PRIVATE_USE_RANGES, UPGRADE_DEADLINE,
};
