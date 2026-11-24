//! Metrology Maven results catalog: a local SQLite index of ResultRecord summaries across
//! registered projects (spec node `RPT.CATALOG.STORE` and its leaves). Modules land one leaf at
//! a time; this crate holds `RPT.CATALOG.STORE.SCHEMA` (module [`schema`] and the catalog
//! location in [`location`]) and the integrity check and content digest of `RPT.CATALOG.CHECK`
//! (module [`check`]; partial, see its documentation).

pub mod check;
pub mod location;
pub mod schema;

pub use check::{
    check_catalog, content_digest, CheckError, CheckReport, EntryKey, ProjectCounts, ProjectSources, RecordLookup,
    SourceRecord, SourceUnreadable,
};
pub use location::{
    catalog_location, catalog_settings, configured_catalog_path, default_catalog_path, CATALOG_PATH_KEY,
};
pub use schema::{
    Availability, Catalog, CatalogEntryRow, CatalogError, CatalogText, DecisionOutcome, EntryStatus, EntryValue,
    EvaluationBasis, EvidenceLabels, Interpretation, Migration, MigrationPlan, Mode, OpenOptions, ProjectRow,
    RecordStatus, UecClass, LATEST, MIGRATIONS, PLAN,
};
