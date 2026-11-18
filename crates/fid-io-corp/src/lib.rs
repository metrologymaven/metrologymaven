//! Format corpora for Metrology Maven. Spec node `IO.CORP.MANIF`: the per-format corpus
//! manifest, its validator (licence rule, digests, coverage of registered formats) and the
//! fetcher for files referenced by URL. The CI binary is `corpus-check`.
//!
//! A manifest `corpus/<format-id>.toml` looks like:
//!
//! ```toml
//! format = "mesh.stl"
//!
//! [[entry]]
//! path = "files/tetra.stl"          # or: url = "https://..."
//! source = "synthetic, make_fixtures.py"
//! licence = "CC-BY-4.0"             # SPDX ID or `unknown`
//! sha256 = "sha256:<64 hex>"
//! expect = { accept = { counts = { facets = 4 } } }   # or { reject = { error = "truncated_file" } }
//! tags = ["p0-subset"]
//! ```
//!
//! `path` is relative to the manifest's directory. Error codes: `manifest_missing`,
//! `licence_not_redistributable`, `digest_mismatch`, `malformed`, plus `io` and `fetch_failed`.
//! Determinism: D0, except fetch outcomes, which are `N` and fail closed.

mod check;
mod error;
mod fetch;
mod manifest;

pub use check::{
    check_corpus, is_redistributable, load_manifests, CheckOptions, Fetching, Report, DEFAULT_MAX_BYTES,
    MAX_MANIFEST_BYTES, REDISTRIBUTION_ALLOWLIST,
};
pub use error::CorpError;
pub use fetch::{cache_path, fetch_to_cache, file_url_to_path, hash_file, hash_reader, FetchError, Fetcher, SystemFetcher};
pub use manifest::{parse_format_listing, Entry, Expect, FormatId, Location, Manifest};
