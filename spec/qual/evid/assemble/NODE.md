---
id: QUAL.EVID.ASSEMBLE
title: Evidence bundle assembly and completeness gate
level: 3
parent: QUAL.EVID
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-QUAL.EVID-002, REQ-QUAL.EVID-003, REQ-QUAL.EVID-005]
covers: []
depends_on: [QUAL.EVID.LAYOUT]
uses_artifacts: [KRN.MAN.MANI, KRN.MAN.VAL, UNC.ORS.RUN, TRUST.REPLAY.CORPUS, KRN.MAN.XART, UNC.RDL.FREEZE, ECO.REL.SIGN.SBOM, QUAL.SBOM.VEX, QUAL.SBOM.SCAN, QUAL.EVID.IMPACT, QUAL.EVID.ANOM, ECO.REL.PROC.GATES, ECO.REL.PROC.RUNBOOK]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.EVID.ASSEMBLE: Evidence bundle assembly and completeness gate

## Purpose

The CI step of the release workflow that collects the generated reports of other domains into the evidence bundle, recomputes every digest, writes the unsigned release-evidence statement and the completeness file `evidence/completeness.json` that `ECO.REL.PROC.GATES` gate `qual-evid` reads before the signing approval step. It copies files and never edits or recomputes their content.

## Scope

**In:**
- Member source map `release/evidence-members.toml`: kind, expected input path, producer node, required flag, redistribution class
- Copy into the bundle layout, SHA-256 per member, statement via `QUAL.EVID.LAYOUT`
- Cross-checks: the kernel manifest digest named in the validation, ORS, XART and corpus reports equals the bundled kernel manifest's digest; the RDL release digest equals the one in the manifest
- Completeness file listing missing, mismatched and restricted members
- Runbook step `evidence-complete` (already in `ECO.REL.PROC.RUNBOOK`) runs this tool

**Out** (and which node owns it instead):
- Producing the members (owned by the producers in `uses_artifacts`)
- Change-impact and anomalies content (owned by `QUAL.EVID.IMPACT`, `QUAL.EVID.ANOM`)
- Blocking the release (owned by `ECO.REL.PROC.GATES`); signing (owned by `ECO.REL.SIGN.CUSTODY`)

## Requirements

- **REQ-QUAL.EVID.ASSEMBLE-001** The member map shall require all eleven member kinds of `QUAL.EVID.LAYOUT` for beta, stable and lts releases.
- **REQ-QUAL.EVID.ASSEMBLE-002** If a required member's input file is absent, then the completeness file shall list it as `missing` and the tool shall exit non-zero.
- **REQ-QUAL.EVID.ASSEMBLE-003** If a report names a kernel manifest digest different from the bundled manifest's digest, or the RDL release digest differs from the manifest's, then the completeness file shall list the member as `mismatched` with both digests.
- **REQ-QUAL.EVID.ASSEMBLE-004** If a member's redistribution class is `restricted`, then the tool shall record its digest and pass status with `published = false` and leave the file out of the public bundle.
- **REQ-QUAL.EVID.ASSEMBLE-005** The tool shall read only files under the given inputs directory, use no clock or network, and produce byte-identical statements and completeness files from identical inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/evid/assemble/complete/` (all 11 inputs, consistent digests), when `cargo xtask qual evidence assemble --release v1.4.0 --channel stable --commit <sha> --inputs complete/ --members evidence-members.toml --out out/` runs, then it exits 0, `out/evidence/completeness.json` lists no problems and `statement.json` equals the golden file.
- **AC-2** (error path) Given `missing-vex/`, then it exits 1 and the completeness file lists `vex.json` as missing (matches `ECO.REL.PROC.GATES` fixture `evid-missing-member/`).
- **AC-3** (error path) Given `wrong-kernel/` (ORS stats name another manifest digest), then `ors_stats` is `mismatched` with both digests.
- **AC-4** Given `restricted/` (validation report class restricted), then the public bundle has no validation report file and the statement lists it with `published = false`.
- **AC-5** (determinism) Given AC-1 run twice on different OSes, then statements are byte-identical.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual evidence assemble --release <tag> --channel <c> --commit <sha> --inputs <dir> --members <toml> --out <dir>` (W1, local output root).
- **Data structures:** `MemberSpec { kind, input, producer, required, redistribution: Public|Restricted }`; `Completeness { schema: "qual-evid-completeness/1", release, status: complete|incomplete, problems: Vec<{kind, path, problem: Missing|Mismatched|Restricted, detail}> }`.
- **Algorithm:** for each spec, stat and hash; extract `kernel_manifest_digest` fields from reports by a per-kind adapter; compare; copy public files; build statement with the layout library.
- **Errors:** `ASM-CONFIG`, `ASM-ADAPTER(kind)` (unparsable report counts as mismatched). Exit 0 complete, 1 incomplete, 2 config.
- **Determinism:** D0.
- **Performance budget:** under 30 s including hashing 2 GB of inputs [unverified].
- **Crate:** `fid-xtask` (module `qual::evid::assemble`).

## Test fixtures

`fixtures/qual/evid/assemble/{complete,missing-vex,wrong-kernel,restricted}/` with small synthetic reports carrying manifest digests; CC-BY-4.0.

## Build prompt

> In `fid-xtask`, add module `qual::evid::assemble`: `qual evidence assemble --release <tag> --channel <c> --commit <sha> --inputs <dir> --members <evidence-members.toml> --out <dir>`. Member map entries: kind, input path, producer, required, redistribution public|restricted. For each: missing → problem; hash; extract the kernel manifest digest from validation, ORS, XART and replay-corpus reports and the RDL release digest, compare with the bundled kernel manifest, mismatch → problem; restricted → keep digest and pass status, published=false, omit file. Write the bundle, `statement.json` via `qual::evid::layout`, and `evidence/completeness.json` (schema `qual-evid-completeness/1`, canonical JSON, sorted). No clock, no network, read only under --inputs; exit 0/1/2. No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.EVID.ASSEMBLE.

## Open questions

- `KRN.MAN.VAL` and `KRN.MAN.MANI` are still open; the adapter field names for their reports are assumptions to confirm (cross-domain).
- TraCIM report redistribution terms are unverified (also raised in `KRN.MAN.VAL`); the default class for the validation report is `restricted` until confirmed.

## Children

_Leaf: none._
