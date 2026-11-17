---
id: TRUST.REPLAY.KART
title: Kernel artifact sourcing for verifier replay
level: 3
parent: TRUST.REPLAY
kind: module
status: open
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-001]
covers: []
depends_on: [KRN.MAN]
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

Once D-020 is decided this is likely one M leaf (lookup chain + cache directory) plus, if option B is chosen, a minor format change in `TRUST.PKG.MANIFEST`.

## Open questions

- **Unblocked (2026-11-17):** D-020 (How the offline verifier obtains historical kernel artifacts) is accepted. Apply its Decision section (`decisions/D-020-*.md`) when this node is expanded.
- Blocked on D-020. Size M is an estimate; it stays non-leaf until the decision fixes the sources.

## Children

_None yet: status `decision` (D-020)._
