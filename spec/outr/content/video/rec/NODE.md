---
id: OUTR.CONTENT.VIDEO.REC
title: Recording procedure and edit-list check
level: 4
parent: OUTR.CONTENT.VIDEO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.VIDEO-001]
covers: []
depends_on: []
uses_artifacts: [VIZ.CAPTURE.VIDEO, VIZ.CAPTURE.EXPORT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.VIDEO.REC: Recording procedure and edit-list check

## Purpose

The written procedure for recording a project video with the platform's own capture commands, and the edit list that is the only allowed way to cut a recording: cuts of journaled video segments and plain title cards, nothing composited from outside the platform. A check confirms that every source in an edit list is a video segment attached in the recording's journal, so a published video can always be traced back to a recording.

## Scope

**In:** the recording procedure page; the `edit.toml` edit-list format; the `video-edl` check.
**Out:** the capture commands, segment format and view track (`VIZ.CAPTURE`); captions and transcripts (`OUTR.CONTENT.VIDEO.CAP`); the published package and hosting (`OUTR.CONTENT.VIDEO.PKG`); encoding tools (a pinned encoder chosen by the procedure, used outside the product).

## Requirements

- **REQ-OUTR.CONTENT.VIDEO.REC-001** The procedure shall instruct authors to record with `capture.start_recording` and `capture.stop_recording` on a published example project and to export segments, view track and journal with `capture.export_media` into an approved local export root.
- **REQ-OUTR.CONTENT.VIDEO.REC-002** An edit list shall contain only `cut` operations (a segment digest with start and end times in seconds) and `title` operations (plain text and a duration of at most 10 s), in playback order.
- **REQ-OUTR.CONTENT.VIDEO.REC-003** If a `cut` names a segment digest that is not attached by a `capture.attach_segment` entry in the package's journal, or whose file is missing or hashes differently, then the check shall report an error naming the operation.
- **REQ-OUTR.CONTENT.VIDEO.REC-004** If an edit list contains any other operation kind, or a cut whose end is not after its start, then the check shall report an error naming the operation.
- **REQ-OUTR.CONTENT.VIDEO.REC-005** The check shall exit 0 with no errors, 1 with any error, and 2 on unreadable input.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/video/edl-ok` (a journal export with three attached segment digests, the three segment files and an edit list of two cuts and one title), when `fid-outreach video-edl --package edl-ok --json` runs, then it exits 0.
- **AC-2** Given fixture `fixtures/outr/video/edl-foreign` (a cut naming a digest not in the journal), when the check runs, then it reports `segment_not_journaled`.
- **AC-3** Given fixture `fixtures/outr/video/edl-overlay` (an `overlay` operation), when the check runs, then it reports `operation_not_allowed`.
- **AC-4** Given fixture `fixtures/outr/video/edl-tampered` (a segment file changed after export), when the check runs, then it reports `segment_digest_mismatch`.
- **AC-5** (determinism) Given AC-1 run twice, then the reports are byte-identical.

## Design

- **Command:** `fid-outreach video-edl --package <dir> [--json]`. Project tooling; nearest class R0.
- **Edit list** (`edit.toml`): `version`, `[[op]]` with `kind = "cut"`, `segment = "sha256:<hex>"`, `start_s`, `end_s`; or `kind = "title"`, `text`, `duration_s`.
- **Journal input:** the exported journal (canonical JSON lines); the check reads only entries whose command ID starts with `capture.attach_segment@` and their segment digest field [unverified field name; fixed by VIZ.CAPTURE.VIDEO].
- **Procedure page:** `content/videos/PROCEDURE.md`: example project, recording settings (1920x1080, 30 fps), export, edit list, encode with the pinned encoder, then captions and packaging.
- **Errors:** findings `segment_not_journaled`, `segment_missing`, `segment_digest_mismatch`, `operation_not_allowed`, `bad_times`, `title_too_long`; fatal `InputUnreadable`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `video_edl`.

## Test fixtures

Synthetic exported journals and small byte files standing in for segments under `fixtures/outr/video/`. CC-BY-4.0.

## Build prompt

> (1) Write `content/videos/PROCEDURE.md`, a short procedure for recording a project video: open a published example project; start recording with `capture.start_recording`, stop with `capture.stop_recording`; export segments, view track and journal with `capture.export_media` into an approved local export root; write an `edit.toml` edit list; encode with the pinned encoder named in the procedure; then add captions and package. Only cuts of recorded segments and plain title cards are allowed.
>
> (2) Build `fid-outreach video-edl --package <dir> [--json]` in the Rust outreach tooling crate `fid-outreach`. Parse `edit.toml` (`version`; `[[op]]` of `kind = "cut"` with `segment = "sha256:<hex>"`, `start_s`, `end_s`, or `kind = "title"` with `text` and `duration_s <= 10`). Read the exported journal and collect segment digests from `capture.attach_segment@*` entries. Report `segment_not_journaled`, `segment_missing`, `segment_digest_mismatch` (streamed SHA-256 of the segment file under `segments/`), `operation_not_allowed`, `bad_times`, `title_too_long`. Exit 0/1/2; RFC 8785 JSON report sorted by operation index.
>
> Acceptance tests under `fixtures/outr/video/` (`edl-ok`, `edl-foreign`, `edl-overlay`, `edl-tampered`, determinism) as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The encoder is an outside tool used by authors; its licence matters only if the project ships it, which this leaf does not.
- VIZ.CAPTURE.REC turns an agent's start request into a proposal for a human; videos are therefore recorded by people.

## Children

_Leaf: none._
