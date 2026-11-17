---
id: OUTR.SITE.DL
title: Download page
level: 3
parent: OUTR.SITE
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-005]
covers: []
depends_on: [OUTR.SITE.GEN, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.SITE.DL: Download page

## Purpose

The download page, generated from the ECO release manifest: every installer and web build of every release, each with its checksum and signature link. It is separate because it is the one page where a wrong link or checksum directly harms users, so it is generated and checked rather than written.

## Scope

**In:**
- Download page per release, generated from the ECO.REL release manifest
- SHA-256 checksums, signature links and verification instructions
- A host allowlist check for every download link

**Out** (and which node owns it instead):
- Building, signing and hosting the artifacts (owned by `ECO.REL`)
- Release notes (owned by `OUTR.SITE.PAGES`)
- Support matrix content (owned by `PLAT.MATRIX`; the page links to it)

## Requirements

- **REQ-OUTR.SITE.DL-001** The download page shall list every installer and web build in the ECO.REL release manifest of each published release, and the build shall fail if the page lists an artifact the manifest does not contain or omits one it does.
- **REQ-OUTR.SITE.DL-002** Each listed artifact shall show its SHA-256 checksum copied from the release manifest and a link to its signature file.
- **REQ-OUTR.SITE.DL-003** If a download or signature link points to a host that is not in the project-controlled host allowlist kept in the site repository, then the site build shall fail and name the link.
- **REQ-OUTR.SITE.DL-004** The download page shall link to instructions for verifying a checksum and a signature on each supported operating system.
- **REQ-OUTR.SITE.DL-005** The download page shall mark the latest release of each channel defined by ECO.REL (for example stable and preview) and show its release date.

## Interfaces

- **Offers:** the download page and per-release anchors (used by blog release notes and docs).
- **Consumes:** ECO.REL release manifest with artifacts, checksums, signatures and channels; OUTR.SITE.GEN imports; PLAT.MATRIX support matrix link.

## Decomposition hints

LIST (manifest to page), HOSTS (allowlist check), VERIFY (verification instructions).

## Open questions

- The release manifest format and channel names belong to ECO.REL, which is not yet expanded.

## Children

_None yet: status `open`._
