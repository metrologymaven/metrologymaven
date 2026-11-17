---
id: OUTR.CONTENT.VIDEO.CAP
title: Captions, transcript and caption review check
level: 4
parent: OUTR.CONTENT.VIDEO
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.VIDEO-002]
covers: []
depends_on: [OUTR.CONTENT.VIDEO.REC]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.VIDEO.CAP: Captions, transcript and caption review check

## Purpose

Every video ships with a WebVTT caption file and a plain-text transcript, both reviewed by a person. This leaf defines the two files and the record of their review, and checks that the captions parse, stay within the video's length, match the transcript, and were reviewed by someone on the roster other than the author.

## Scope

**In:** caption and transcript file rules; the caption review record; the `video-captions` check.
**Out:** recording and edit list (`OUTR.CONTENT.VIDEO.REC`); packaging and hosting (`OUTR.CONTENT.VIDEO.PKG`); speech-to-text tools (authors may use any local tool; output is always reviewed).

## Requirements

- **REQ-OUTR.CONTENT.VIDEO.CAP-001** Each video package shall contain `captions.vtt` in WebVTT format and `transcript.txt` in UTF-8.
- **REQ-OUTR.CONTENT.VIDEO.CAP-002** If the caption file does not parse, has overlapping cues, or has a cue ending after the video's length as given by the edit list, then the check shall report an error naming the cue.
- **REQ-OUTR.CONTENT.VIDEO.CAP-003** If the transcript, with whitespace collapsed, does not equal the concatenated cue texts with whitespace collapsed, then the check shall report `transcript_mismatch`.
- **REQ-OUTR.CONTENT.VIDEO.CAP-004** If the package has no caption review record naming a reviewer on the roster who is not the video's author, with the SHA-256 of both files, or the digests do not match, then the check shall report an error.
- **REQ-OUTR.CONTENT.VIDEO.CAP-005** The check shall exit 0 with no errors, 1 with any error, and 2 on unreadable input.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/video/cap-ok` (edit list of 42 s, 12 non-overlapping cues, matching transcript, review record by a roster member), when `fid-outreach video-captions --package cap-ok --roster roster.txt --author alice --json` runs, then it exits 0.
- **AC-2** Given fixture `cap-overlap`, when the check runs, then it reports `cue_overlap` with the cue index.
- **AC-3** Given fixture `cap-too-long` (a cue ending at 45 s), when the check runs, then it reports `cue_beyond_end`.
- **AC-4** Given fixture `cap-transcript` (one word changed in the transcript), when the check runs, then it reports `transcript_mismatch`.
- **AC-5** Given fixture `cap-self-review` (reviewer equals author), when the check runs, then it reports `self_review`.

## Design

- **Command:** `fid-outreach video-captions --package <dir> --roster <file> --author <id> [--json]`. Project tooling; nearest class R0.
- **Review record** (`caption-review.toml`): `reviewer`, `date`, `captions_sha256`, `transcript_sha256`.
- **Video length:** the sum of cut durations and title durations from `edit.toml` (parsed with the REC module).
- **Parsing:** a bounded WebVTT subset (header, cues with `HH:MM:SS.mmm --> HH:MM:SS.mmm`, text lines; notes and styles ignored); files over 2 MB refused.
- **Errors:** findings `vtt_parse`, `cue_overlap`, `cue_beyond_end`, `transcript_mismatch`, `review_missing`, `review_digest_mismatch`, `unknown_reviewer`, `self_review`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `video_captions`.

## Test fixtures

Hand-written caption, transcript, edit-list and review fixtures under `fixtures/outr/video/`. CC-BY-4.0.

## Build prompt

> Build `fid-outreach video-captions --package <dir> --roster <file> --author <id> [--json]` in the Rust outreach tooling crate `fid-outreach`. In `<dir>`: `captions.vtt` (parse a bounded WebVTT subset; refuse files over 2 MB), `transcript.txt`, `edit.toml` (video length = sum of cut and title durations; reuse the crate's `video_edl` parser), `caption-review.toml` (`reviewer`, `date`, `captions_sha256`, `transcript_sha256`). Report `vtt_parse`, `cue_overlap`, `cue_beyond_end`, `transcript_mismatch` (whitespace-collapsed transcript vs concatenated cue texts), `review_missing`, `review_digest_mismatch`, `unknown_reviewer` (roster is one handle per line), `self_review` (case-insensitive). Exit 0/1/2; RFC 8785 JSON report.
>
> Acceptance tests under `fixtures/outr/video/` (`cap-ok`, `cap-overlap`, `cap-too-long`, `cap-transcript`, `cap-self-review`) as listed in the spec.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- Languages other than English are out of launch-1; additional caption tracks per language would add one file and one record each.

## Children

_Leaf: none._
