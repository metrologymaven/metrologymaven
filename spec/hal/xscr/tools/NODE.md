---
id: HAL.XSCR.TOOLS
title: Transcript inspection, trimming, redaction and export
level: 3
parent: HAL.XSCR
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.XSCR-002]
covers: []
depends_on: [HAL.XSCR.FMT, CORE.JRN, CORE.STO]
side_effect: W2
determinism: D0
cli: [fid device show-transcript, fid device trim-transcript, fid device export-transcript]
mcp: [device_show_transcript, device_trim_transcript, device_export_transcript]
journal: [device.show_transcript@1, device.trim_transcript@1, device.export_transcript@1]
updated: 2026-11-16
---

# HAL.XSCR.TOOLS: Transcript inspection, trimming, redaction and export

## Purpose

The user-facing commands for transcripts: show a transcript and match its entries to journal entries by trace ID, trim it to a range as a new transcript, and export it as a local bug bundle with optional redaction of serial numbers and untrusted text. These are what a user or maintainer needs to turn a field problem into a reproducible case.

## Scope

**In:**
- `device.show_transcript@1` (R0): header, segment digests, entry counts, and a per-journal-entry view joined by trace ID
- `device.trim_transcript@1` (W2): new transcript containing an entry range, with a new header that references the source transcript digest; the source is never modified
- `device.export_transcript@1` (W1, `fs_effect: write_root` per CONSTITUTION §3.1): a bug bundle (transcript, referenced payloads, versions) written into an approved local export root, with optional redaction

**Out:**
- Sending a bundle off the machine (X1; owned by `AUTO` and `ECO` support tooling)
- Deleting transcripts (owned by `CORE.STO` retention)

## Requirements

- **REQ-HAL.XSCR.TOOLS-001** When `device.show_transcript@1` runs with a journal entry ID, the system shall return exactly the transcript entries whose trace ID equals that entry's trace ID, in transcript order.
- **REQ-HAL.XSCR.TOOLS-002** When `device.trim_transcript@1` runs with an entry range, the system shall write a new transcript containing exactly that range, whose header references the source transcript's final segment digest, and shall leave the source unchanged.
- **REQ-HAL.XSCR.TOOLS-003** When `device.export_transcript@1` runs with `--redact`, every device serial and every `UntrustedText` field shall be replaced by a salted SHA-256 token that is stable within the bundle, and the bundle manifest shall list which fields were redacted.
- **REQ-HAL.XSCR.TOOLS-004** The export command shall write only into an approved local export root given as a handle, and shall refuse raw paths with `path_not_permitted`.
- **REQ-HAL.XSCR.TOOLS-005** If a trim range is empty or outside the transcript, then trimming shall fail with `trim_range_invalid` and write nothing.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/xscr/tools/gauge_session.xscr` and its journal, when `fid device show-transcript --entry <journal entry 7> --json` runs, then the output lists exactly the 4 transcript entries with that trace ID.
- **AC-2** Given the same transcript, when `fid device trim-transcript --from 10 --to 40 --json` runs, then a new transcript of 31 entries is created, its header references the source digest, and the source file digest is unchanged.
- **AC-3** Given the same transcript, when `fid device export-transcript --redact --root <test root handle> --json` runs, then the bundle contains no serial string from the fixture, equal serials map to equal tokens, and the manifest lists the redacted fields.
- **AC-4** (error path) Given a raw path instead of a root handle, when export runs, then it fails with `path_not_permitted` and writes nothing.
- **AC-5** (error path) Given `--from 50 --to 40`, when trim runs, then it fails with `trim_range_invalid` and journals nothing.
- **AC-6** (replay) Given the journal of AC-2, when replayed in exact mode, then the outcome is `IDENTICAL` and the trimmed transcript digest matches.

## Design

- **Registry commands:** `device.show_transcript@1` (R0; params: transcript handle, optional journal entry ID or entry range); `device.trim_transcript@1` (W2; params: transcript handle, from, to; result: new transcript handle); `device.export_transcript@1` (W1, `fs_effect: write_root`; params: transcript handle, export-root handle, `redact: bool`, salt generated per bundle and kept out of the bundle). The frontmatter `side_effect` lists the highest class (W2).
- **Data structures:** `BugBundle { manifest, transcript, payloads, versions }`, `RedactionManifest`.
- **Algorithm:** streaming read with `HAL.XSCR.FMT`; trim re-writes with the same writer; redaction walks canonical JSON bodies by schema-declared field kinds.
- **Errors:** `trim_range_invalid`, `path_not_permitted`, reader errors.
- **Determinism:** D0 for show and trim; export is D0 for a given salt.
- **Performance budget:** show over a 1 GB transcript in < 5 s on reference hardware [unverified until measured].
- **Crate:** `fid-hal-xscr` (module `tools`).

## Test fixtures

`fixtures/hal/xscr/tools/` holds a gauge-simulator transcript with invented serials, its journal and payloads (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-xscr`, module `tools`, implement three registry commands: `device.show_transcript@1` (R0; CLI `fid device show-transcript`, MCP `device_show_transcript`) returning header, segment digests, counts, and the entries matching a journal entry's trace ID; `device.trim_transcript@1` (W2; CLI `fid device trim-transcript`, MCP `device_trim_transcript`) writing a new transcript for an inclusive entry range whose header references the source digest, failing `trim_range_invalid` on bad ranges; `device.export_transcript@1` (W1 with `fs_effect: write_root`; CLI `fid device export-transcript`, MCP `device_export_transcript`) writing a bug bundle (transcript, payloads, versions, manifest) into an approved export-root handle, refusing raw paths with `path_not_permitted`, and with `--redact` replacing serials and `UntrustedText` with stable salted SHA-256 tokens listed in the manifest. Use `HAL.XSCR.FMT` reader and writer. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures in `fixtures/hal/xscr/tools/`.

## Open questions

- The parent lists `device.export_transcript@1` as R0; CONSTITUTION §3.1 (D-007) assigns local export-root writes to W1 with `fs_effect: write_root`, which this leaf follows.
- Redaction may not be enough for transcripts of NDA vendor hosts (parent open question).

## Children

_Leaf: none._
