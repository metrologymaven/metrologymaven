---
id: IO.RES.CSV.MAP
title: Versioned result column maps and table dialect
level: 4
parent: IO.RES.CSV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-io-res]
satisfies: [REQ-IO.RES.CSV-001, REQ-IO.RES.CSV-003]
covers: []
depends_on: [CORE.JRN.CANON, IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.RES.CSV.MAP: Versioned result column maps and table dialect

## Purpose

A column map says which column of a results table holds which field (characteristic ID, nominal, actual, limits, unit, status, evaluation basis, uncertainty class, decision rule, record status, source software and so on), plus the table dialect (delimiter, decimal mark, header, quoting). Maps are data: versioned, digested and journaled, with built-in maps for common layouts and user maps loaded from import roots. This leaf also decides when an export map is `lossy`.

## Scope

**In:**
- Column-map schema `colmap@1` (JSON), map ID, semantic version, SHA-256 of canonical JSON
- Field vocabulary with types (text, decimal, enum) and `required` flags
- Dialect: delimiter (comma, semicolon, tab), decimal mark (`.` or `,` only with a non-comma delimiter), header row, RFC 4180 quoting, encoding UTF-8 (BOM accepted on read)
- Built-in maps: `fid.results.long@1` (one row per result, every field), `fid.results.wide@1` (one row per part, one column group per characteristic)
- Lossy check: the list of ResultRecord fields an export map omits

**Out:**
- Reading and writing files (`IO.RES.CSV.IN`, `IO.RES.CSV.OUT`)
- Point tables and their dialect (`IO.CLOUD.ASCII`, P1b; this leaf states the same dialect rules independently)

## Requirements

- **REQ-IO.RES.CSV.MAP-001** Every column map shall carry an ID, a version and a SHA-256 of its RFC 8785 canonical form, and these three values shall be available for journaling.
- **REQ-IO.RES.CSV.MAP-002** If a map names an unknown field, repeats a column, or combines decimal comma with comma delimiter, then loading shall fail with `colmap_invalid` naming the problem.
- **REQ-IO.RES.CSV.MAP-003** For an export map, the lossy check shall list every field among `evaluation_basis`, uncertainty class, decision rule, record status and source software that the map omits, and an empty list shall mean not lossy.
- **REQ-IO.RES.CSV.MAP-004** The built-in maps shall include every field of the lossy check, so exports with them are never lossy.
- **REQ-IO.RES.CSV.MAP-005** A user map file shall be read under `IO.REG.LIMITS` with a 1 MiB cap and its text fields typed as untrusted.

## Acceptance criteria

- **AC-1** Given `fixtures/io/res/csv/map/user_map.json`, when loaded, then ID, version and digest equal `user_map.expected.json`, and reordering its JSON keys gives the same digest.
- **AC-2** (error path) Given `dup_column.json`, `unknown_field.json` and `comma_comma.json`, then each fails `colmap_invalid` with its reason.
- **AC-3** Given an export map without `decision_rule` and `record_status`, then the lossy list is exactly those two.
- **AC-4** Given the built-in maps, then their lossy lists are empty.

## Design

- **Registry commands:** none (library; maps are journaled by the commands that use them).
- **Data structures:** `ColumnMap { id, version, dialect: Dialect, columns: Vec<Column { header?, index?, field: Field, required }> }`; `Field` enum (characteristic_id, characteristic_name, nominal, actual, deviation, lower_limit, upper_limit, unit, status, evaluation_basis, uec, decision_rule, record_status, source_software, source_version, part_id, timestamp_text, comment).
- **Algorithm:** bounded JSON parse → validation → canonicalise with `fid-canon` → digest.
- **Errors:** `colmap_invalid{reason}`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-io-res` (module `csv::map`).

## Test fixtures

`fixtures/io/res/csv/map/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-res`, add module `csv::map`: column-map schema `colmap@1` (ID, version, dialect, columns with field, header or index, required), field vocabulary for result tables, dialect rules (delimiter comma/semicolon/tab, decimal `.` or `,` only with a non-comma delimiter, header, RFC 4180 quoting, UTF-8 with optional BOM on read), digest of the RFC 8785 canonical form via `fid-canon`, validation errors `colmap_invalid{reason}`, a lossy check over evaluation basis, UEC, decision rule, record status and source software, and built-in maps `fid.results.long@1` and `fid.results.wide@1` that are never lossy. User maps ≤ 1 MiB, untrusted text. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
