---
id: TRUST.REPLAY.KART
title: Kernel artifact sourcing for verifier replay
level: 3
parent: TRUST.REPLAY
kind: module
status: split
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-001]
covers: []
depends_on: []
uses_artifacts: [KRN.MAN.WASM, KRN.MAN.MANI.SCHEMA, KRN.MAN.REPRO]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.REPLAY.KART: Kernel artifact sourcing for verifier replay

## Purpose

Finds the exact WASM reference kernel artifact named by a package's kernel manifest and hands its bytes to the sandbox, which checks the hash. It implements the `ArtifactSource` trait that `TRUST.REPLAY.SELECT` asks for availability. Where artifacts come from (bundled with the verifier, carried in the package, a local cache filled by opt-in download or reproducible rebuild) is a maintainer choice recorded in D-020.

## Scope

**In:**
- `ArtifactSource` trait implementation and lookup order
- Reporting the artifact's source in the replay output
- Whatever sources D-020 approves (bundled set, in-package member, local cache, opt-in fetch)

**Out** (and which node owns it instead):
- Hash check and execution (`TRUST.REPLAY.SANDBOX`)
- Producing and archiving artifacts (`KRN.MAN`, `ECO`)

## Requirements

- **REQ-TRUST.REPLAY.KART-001** When asked for the artifact of a kernel manifest digest, the source shall return the artifact bytes and their origin, or `KERNEL_UNAVAILABLE` naming the kernel version.
- **REQ-TRUST.REPLAY.KART-002** The source shall perform no network access unless the user passed an explicit opt-in flag for that run, and any such access shall be class X1.
- **REQ-TRUST.REPLAY.KART-003** The lookup order and the permitted sources shall follow D-020.

## Interfaces

- **Offers:** `ArtifactSource::get(kernel_manifest_digest) -> Result<(Vec<u8>, Origin), Unavailable>`.
- **Consumes:** kernel manifests and artifacts as published by `KRN.MAN`.

## Decomposition hints

Per D-020 option C: an offline lookup chain inside the verifier (LOOKUP) and an explicit, opt-in X1 fetch command that fills the local cache from the project's release archive (FETCH). The fetch lives in `fid`, not `fid-verify`, so the verifier binary keeps no network-capable dependency (REQ-TRUST.VERIFY.CLI-001).

## Open questions

- D-020 (accepted 2026-11-17) chose option C: bundled current-major artifacts, an in-package artifact when present (hash-checked), an explicit opt-in fetch from the project's release archive, and a documented reproducible rebuild path; the artifact's origin is reported. Applied by the children below.
- Nothing yet writes an in-package kernel artifact. `TRUST.REPLAY.KART.LOOKUP` accepts a manifest-listed member `kernel/<sha256>.wasm`; adding an export option for it belongs to `TRUST.PKG.CONTENT` (a later minor format addition, documented by `TRUST.PKG.FIXTURE`).
- The project's release-archive URL depends on D-002 (project name) and ECO.REL hosting; FETCH reads it from configuration.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.REPLAY.KART.LOOKUP | Offline kernel artifact lookup chain | 001 003 | none |
| TRUST.REPLAY.KART.FETCH | Opt-in kernel artifact fetch into the local cache | 002 003 | none |
