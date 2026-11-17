---
id: D-020
title: How the offline verifier obtains historical kernel artifacts
status: open
raised_by: TRUST.REPLAY split
blocks: [TRUST.REPLAY.KART]
decide_by: before the first kernel release after 1.0
updated: 2026-11-16
---

# D-020: How the offline verifier obtains historical kernel artifacts

## Question

Verifier replay (REQ-TRUST-003) must run the WASM reference kernel whose version matches the package's kernel manifest. A package produced years ago names an old kernel. Where does `fid-verify` get that exact artifact while staying offline (C7) and free (C8)?

## Context

KRN.MAN produces a byte-identical WASM artifact per kernel release with its hash in the kernel manifest (REQ-KRN.MAN-004, -009). `TRUST.REPLAY.SANDBOX` checks the artifact hash before running it (REQ-TRUST.REPLAY-009). What is still open is distribution: it affects package size, verifier download size, offline guarantees and long-term reproducibility (artifacts can always be rebuilt from source because builds are reproducible, but rebuilding needs a toolchain). Blocks `TRUST.REPLAY.KART`.

## Options

| Option | For | Against |
|---|---|---|
| A. Bundle every released kernel artifact in each `fid-verify` release | Fully offline; simple | Verifier grows with every kernel release [unverified: expected artifact size a few MB each] |
| B. Include the kernel artifact inside each Result Package (optional member `kernel/kernel.wasm`) | Package is self-contained forever | Every package grows by the artifact size; duplicated across packages |
| C. Local artifact cache directory filled by the user: from a bundled set (A for recent releases), from a package (B when present), or by an explicit opt-in download (X1) or a local reproducible rebuild | Flexible; offline by default; no forced growth | More rules; a missing artifact gives `NOT_REPLAYABLE` until the user acts |

## Recommendation

C: bundle the current major's artifacts, accept an in-package artifact when present (hash-checked like any other), allow an explicit opt-in fetch from the project's release archive, and document the reproducible rebuild path. Report the artifact's source in the replay report.

## Decision

_Open._
