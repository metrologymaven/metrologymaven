---
id: VIZ.WEB.PKG
title: ViewerPackage format
level: 3
parent: VIZ.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-004, REQ-VIZ.WEB-008]
covers: []
depends_on: [TRUST.PKG, TRUST.SIGN, VIZ.VIEW.STATE]
cli: [fid view inspect-package]
mcp: [view_inspect_package]
journal: [view.inspect_package@*]
updated: 2026-11-16
---

# VIZ.WEB.PKG: ViewerPackage format

## Purpose

The content-addressed, signed container the browser viewer reads: parts for tessellations, cloud LOD, deviation display data (f32 display copy, u8 kernel bins and an f64 sidecar for readouts), legends, NamedViews and a characteristics projection; plus kernel manifest reference, journal head, the explicit redaction list and signature envelopes. And the R0 command that inspects a package's structure.

## Scope

**In:**
- Format definition and versioning (`fiducial.viewer-package/1`)
- Part roles, media types and digests
- f64 sidecar for every number the viewer shows, with kernel-formatted strings
- Redaction list
- `view.inspect_package@1` (R0)

**Out** (and which node owns it instead):
- Signature envelopes and verification (`TRUST.SIGN`, `TRUST.VERIFY`)
- Result Package for records (`TRUST.PKG`; the ViewerPackage references it)
- Writing packages (`VIZ.WEB.EXPORT`); parsing in the browser (`VIZ.WEB.LOADER`)

## Requirements

- **REQ-VIZ.WEB.PKG-001** Every part of a ViewerPackage shall be listed with role, media type and SHA-256 digest, and the manifest shall carry the kernel manifest digest and journal head.
- **REQ-VIZ.WEB.PKG-002** Every number the viewer can display shall be present in the package as an f64 value together with its kernel-formatted string and formatter ID.
- **REQ-VIZ.WEB.PKG-003** Every redaction applied at export shall be listed in the manifest with its scope.
- **REQ-VIZ.WEB.PKG-004** `view.inspect_package@1` shall report parts, digests, redactions, signature presence and format version without rendering.
- **REQ-VIZ.WEB.PKG-005** The format shall declare size limits per part and in total that a loader can enforce.

## Interfaces

- **Offers:** ViewerPackage format (consumed by RPT, TRUST verifier, OUTR); `view.inspect_package@1`.
- **Consumes:** `TRUST.PKG` conventions; `TRUST.SIGN` envelopes; `VIZ.VIEW.STATE` for NamedViews.

## Decomposition hints

MANIFEST, PARTS, SIDECAR (f64 and formatted strings), REDACT, INSPECT.

## Open questions

- Whether the ViewerPackage is a profile of the TRUST Result Package or a separate container is to be agreed with TRUST.PKG.

## Children

_None yet: status `open`._
