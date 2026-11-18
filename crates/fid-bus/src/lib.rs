//! The command bus of Metrology Maven: every surface invokes commands through it
//! (CONSTITUTION C3). Modules land one spec leaf at a time; see `spec/core/bus/`.

pub mod error;

/// Short alias for [`error`].
pub use self::error as err;

pub use error::{
    bus_error_schema, bus_errors_digest, bus_errors_json, catalog_value, check_compatible, current_catalog,
    parse_catalog, BusError, Catalog, CatalogError, Code, Detail, Removal, Untrusted, MAX_DETAIL_CHARS, SCHEMA_VERSION,
};
