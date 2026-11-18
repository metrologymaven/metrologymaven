# Pinned DMSC QIF 3.0 schema set (TODO: not yet populated)

This directory is where the real QIF 3.0 XSD and XSLT files and their
`qif-xsd.manifest.toml` belong (spec node IO.QIF.GEN.PIN, AC-5). They are **not
committed yet**: the upstream licence and redistribution terms have not been checked.

## TODO for a maintainer (a person, not an agent)

1. Confirm the licence of the upstream `qif-community` repository (believed to be Boost
   Software License 1.0 [unverified]) and that it allows redistribution in this
   repository. If it does not, stop: record the outcome in a decision (`decisions/`).
2. Pick an upstream tag or commit. Copy the QIF 3.0 `.xsd` files, the normative XSLT
   files and the upstream licence file into this directory, keeping upstream's
   relative layout and bytes exactly (no line-ending changes; see `.gitattributes`).
3. Write `qif-xsd.manifest.toml` here (format in `crates/fid-qif-gen/src/pin.rs`):
   `manifest_version = 1`, `licence_path`, `licence_sha256`, `[source] url, rev`, and
   one `[[file]]` row (`path`, `bytes`, `sha256`, `role`) per schema file. Compute sizes
   and digests with an independent tool (for example `sha256sum` or Python `hashlib`).
4. Remove the `#[ignore]` from `ac5_real_qif3_set_verifies` in `tests/pin.rs` so CI
   verifies the set on Linux, Windows and macOS.

The build never fetches these files from the network.
