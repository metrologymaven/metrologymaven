//! Result Package format of Metrology Maven. Spec node `TRUST.PKG.MANIFEST`.
//!
//! A Result Package is a ZIP container (zip64 when needed) with a fixed member order:
//!
//! 1. `manifest.json`: [`Manifest`], every member's path, media type, byte length and SHA-256;
//! 2. the members, sorted by the UTF-8 bytes of their paths, each under one of the
//!    reserved folders [`MEMBER_DIRS`];
//! 3. `statement.json`: an in-toto Statement v1 ([`Statement`]) whose subjects are
//!    `manifest.json` plus every member.
//!
//! DSSE envelopes under `signatures/` are added later by `TRUST.SIGN` and are never listed
//! in the manifest. Both JSON files are RFC 8785 canonical JSON (through `fid-canon`) with
//! no trailing newline, and every container byte is a function of the logical contents
//! only (determinism D0): fixed timestamps, no comments, no extra fields except zip64, and
//! the compression method chosen from the media type alone.
//!
//! Errors are one enum, [`PkgError`], with stable codes `pkg.*`.
//!
//! Reading a package from untrusted bytes goes through [`reader`] (spec node
//! `TRUST.PKG.READER`): the bounded, fail-closed [`reader::PackageReader`], its error type
//! [`reader::ReadError`] and the read-only command `verify.inspect_package@1`.

pub mod container;
mod error;
pub mod manifest;
mod path;
pub mod reader;
mod version;

pub use container::{
    build_package, build_package_with, crc32, method_for_media_type, BuiltPackage, MemberInput, Method, PackageInput,
    Zip64, STORED_MEDIA_TYPES,
};
pub use error::PkgError;
pub use manifest::{
    check_media_type, check_member_path, check_members, hash_bytes, hash_reader, EvidenceTier, Manifest, Member,
    PackagePredicate, Statement, Subject, FORMAT_NAME, JSON_MEDIA_TYPE, MANIFEST_PATH, MEMBER_DIRS, PREDICATE_TYPE,
    SIGNATURES_DIR, STATEMENT_PATH, STATEMENT_TYPE,
};
pub use path::{PkgPath, MAX_PATH_BYTES};
pub use version::{check_compatible, SemVer, FORMAT_VERSION};
