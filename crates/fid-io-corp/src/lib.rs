//! Format corpora for Metrology Maven. Spec node `IO.CORP.MANIF`: the per-format corpus
//! manifest, its validator (licence rule, digests, coverage of registered formats) and the
//! fetcher for files referenced by URL. The CI binary is `corpus-check`. Spec node
//! `IO.CORP.HOSTILE`: the hostile corpus generator (module docs in `hostile`), CI binary
//! `corpus-hostile`.
//!
//! A manifest `corpus/<format-id>.toml` looks like:
//!
//! ```toml
//! format = "mesh.stl"
//!
//! [[entry]]
//! path = "files/tetra.stl"          # or: url = "https://..." (http:// or https:// only)
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
mod hostile;
mod manifest;

pub use check::{
    check_corpus, is_redistributable, load_manifests, CheckOptions, Fetching, Report, DEFAULT_MAX_BYTES,
    MAX_MANIFEST_BYTES, REDISTRIBUTION_ALLOWLIST,
};
pub use error::CorpError;
pub use fetch::{
    cache_path, fetch_to_cache, hash_file, hash_reader, is_fetchable_url, FetchError, Fetcher, SystemFetcher,
};
pub use hostile::{
    find_xml_root, generate, mutators_for_tags, parse_plans, render_manifest, rng_stream, run_hostile,
    truncate_lengths, zip_bomb, CountField, FlipRange, HostileEntry, HostileOptions, HostileReport, Mutator,
    MutatorPlan, Override, Variant, BITFLIPS, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_TOTAL_BYTES, MAX_BOMB_BYTES,
    NEST_DEPTH,
};
pub use manifest::{parse_format_listing, Entry, Expect, FormatId, Location, Manifest};
