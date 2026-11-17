---
id: PLAT.DOCTOR.REPORT
title: Doctor report schema, text rendering and redaction
level: 3
parent: PLAT.DOCTOR
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-003, REQ-PLAT.DOCTOR-008]
covers: []
depends_on: [PLAT.DOCTOR.PROBE, PLAT.DOCTOR.LOOKUP]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DOCTOR.REPORT: Report schema, rendering and redaction

## Purpose

Defines the doctor report: a versioned JSON document holding the snapshot and the assessment, the human-readable text rendering of the same content, and the redaction pass that makes a report safe to paste into a public bug report. JSON and text are two views of one value, so they can never disagree.

## Scope

**In:**
- `DoctorReport` type and its JSON Schema (`schema_version` 1)
- Text rendering (sections: platform, hardware, GPU, verdicts, unknowns)
- Redaction of identifying strings, on by default
- Parsing a report back (for `show-report`) with bounded input

**Out** (and which node owns it instead):
- Gathering and assessing (`PLAT.DOCTOR.PROBE`, `PLAT.DOCTOR.LOOKUP`)
- The commands and flags (`PLAT.DOCTOR.CMD`)

## Requirements

- **REQ-PLAT.DOCTOR.REPORT-001** The report JSON shall carry `schema_version`, tool version, build phase, the snapshot, the assessment and the outcome, and shall validate against the published JSON Schema.
- **REQ-PLAT.DOCTOR.REPORT-002** The text rendering shall contain every item, verdict, shortfall and unknown reason present in the JSON, so that a test can check each JSON leaf value appears in the text.
- **REQ-PLAT.DOCTOR.REPORT-003** By default the redaction pass shall replace the current user name, host name, home directory and any absolute path outside the install directory with `<redacted:user>`, `<redacted:host>`, `<redacted:home>` or `<redacted:path>` in every string of the report.
- **REQ-PLAT.DOCTOR.REPORT-004** When the caller passes `include_identifying = true`, the redaction pass shall be skipped and the report shall carry `redacted: false`.
- **REQ-PLAT.DOCTOR.REPORT-005** If a report given for parsing exceeds 1 MiB, has an unsupported `schema_version` or fails schema validation, then parsing shall fail with a typed error.
- **REQ-PLAT.DOCTOR.REPORT-006** When rendering a parsed report, every string shall be rendered as data with control and escape characters replaced, per CONSTITUTION C6.

## Acceptance criteria

- **AC-1** Given the assessment from `PLAT.DOCTOR.LOOKUP` AC-1 and its snapshot, when rendered to JSON, then it validates against `platform/doctor-report.schema.json` and equals `fixtures/plat/doctor/report-win11.json` byte for byte.
- **AC-2** Given the same report, when rendered to text, then the output equals `fixtures/plat/doctor/report-win11.txt`, and every JSON leaf value appears in it.
- **AC-3** Given a snapshot whose `detail` strings contain the test's user name, host name and `C:\Users\<name>\data`, when redacted, then none of those substrings remain and the placeholders appear.
- **AC-4** Given `include_identifying = true`, when rendered, then the strings are unchanged and `redacted` is false.
- **AC-5** (error path) Given `fixtures/plat/doctor/report-schema99.json`, when parsed, then the result is `UnsupportedSchema`.
- **AC-6** Given `fixtures/plat/doctor/report-escape.json` containing ANSI escape sequences in a detail string, when rendered to text, then no escape byte (0x1B) appears in the output.

## Design

- **Registry command(s):** none; library used by `PLAT.DOCTOR.CMD`.
- **Data structures:** `DoctorReport { schema_version: 1, tool_version, phase, redacted: bool, snapshot: EnvSnapshot, assessment: Assessment }`. No timestamp in the report body so the same machine state gives the same report; the command may add one in its human header only.
- **Algorithm:** serde with fixed field order; JSON Schema generated from types; text renderer walks the same structure; redaction gathers identifying strings from the OS at call time (user, host, home) and replaces them, then replaces absolute paths not under the install directory by pattern.
- **Errors:** `TooLarge`, `UnsupportedSchema`, `SchemaInvalid { pointer }`, `Parse`.
- **Determinism:** D0 for render and parse.
- **Performance budget:** trivial.
- **Crate:** `fid-doctor` (module `report`).

## Test fixtures

Golden JSON and text files under `fixtures/plat/doctor/`, derived from the PROBE and LOOKUP fixtures; licensed with the code.

## Build prompt

> In crate `fid-doctor`, add module `report`: `DoctorReport { schema_version: 1, tool_version, phase, redacted, snapshot, assessment }` with a fixed field order, a generated JSON Schema written by `xtask doctor-schema` to `platform/doctor-report.schema.json`, `to_json`, `to_text` (sections Platform, Hardware, GPU, Verdicts, Unknowns; every JSON leaf value appears), `redact(report, install_dir)` replacing user name, host name, home directory and absolute paths outside the install directory with `<redacted:user|host|home|path>` (on by default; skipped when `include_identifying`), and `parse(bytes)` with a 1 MiB cap, schema-version check and schema validation. Text rendering escapes control characters (CONSTITUTION C6: report content is untrusted data).
>
> Acceptance tests: the six criteria above with fixtures in `fixtures/plat/doctor/`. Constraints: CONSTITUTION §8. Commit: `feat(fid-doctor): report schema, rendering and redaction`.

## Open questions

- None.

## Children

_Leaf: none._
