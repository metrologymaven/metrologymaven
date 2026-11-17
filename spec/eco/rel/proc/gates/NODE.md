---
id: ECO.REL.PROC.GATES
title: Aggregated release gate
level: 4
parent: ECO.REL.PROC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PROC-001, REQ-ECO.REL.PROC-002]
covers: []
depends_on: [ECO.REL.PROC.VERSION]
uses_artifacts: [QUAL.EVID, QUAL.THREAT, QUAL.CRA, QUAL.SBOM, AGENT.REDTEAM, PLAT.MATRIX.CI, FLOW.RUNNER, I18N.XLATE, KRN.MAN.XART, ECO.REL.PKG.PLAN, ECO.REL.SIGN.OSSIGN, ECO.REL.LTS, ECO.REL.PROC.CHANNEL]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PROC.GATES: Aggregated release gate

## Purpose

One pure function from a set of gate files to one release verdict. Every domain that can stop a release (QUAL, AGENT, PLAT, FLOW, I18N, KRN and ECO.REL's own packaging, OS signing and LTS checks) publishes a verdict file; this tool reads them all, applies the rules in `release/gates.toml`, and writes `release-gates.json`. The release workflow will not ask a person to approve signing unless the verdict is `pass`, and it publishes only to the venues the verdict enables.

## Scope

**In:**
- `release/gates.toml`: gate ID, producer node, expected file path, the release kinds and channels it applies to, and whether a missing file blocks
- One adapter per gate file format, each reducing it to `pass`, `block(reasons)` or `not_applicable`
- The EU venue rule: venues flagged `eu_distribution` are disabled, not the whole release, when the CRA counsel gate fails
- `cargo xtask rel gate`

**Out:**
- Producing any gate file (owned by the producer nodes listed in `uses_artifacts`)
- Running the workflow and stopping at approval (owned by `ECO.REL.PROC.FLOW`)

## Requirements

- **REQ-ECO.REL.PROC.GATES-001** If any applicable gate evaluates to `block`, or an applicable gate file whose `missing_blocks` is true is absent, then the verdict shall be `block` and shall name each gate and reason.
- **REQ-ECO.REL.PROC.GATES-002** If the evidence-bundle completeness file from `QUAL.EVID` reports any missing or mismatched member, then gate `qual-evid` shall block and name each member.
- **REQ-ECO.REL.PROC.GATES-003** When the release is the first release of a `MAJOR.MINOR` (patch 0, any suffix), gate `qual-threat` shall block unless a threat-model review record for that `MAJOR.MINOR` exists in the `QUAL.THREAT` review directory.
- **REQ-ECO.REL.PROC.GATES-004** If the `QUAL.CRA` support-period statement or classification record does not have review status `counsel_reviewed`, then gate `qual-cra` shall disable every venue whose `eu_distribution` flag is true and list them, without blocking publication to other venues.
- **REQ-ECO.REL.PROC.GATES-005** If the `AGENT.REDTEAM` verdict file is missing, names a different release candidate than the one being gated, or has verdict `block`, then gate `agent-redteam` shall block and copy the named tasks, agents and journal entries into the reasons.
- **REQ-ECO.REL.PROC.GATES-006** The gate set shall also include the vulnerability gate (`QUAL.SBOM`), the platform gate (`PLAT.MATRIX.CI`), the package-build gate (`ECO.REL.PKG.PLAN`), the OS-signing readiness gate (`ECO.REL.SIGN.OSSIGN`), the per-app GA gates (`FLOW.RUNNER`) for every app labelled `ga`, the translation gate (`I18N.XLATE`), the kernel patch bit-identity gate (`KRN.MAN.XART`) for patch releases, and the LTS gate (`ECO.REL.LTS`) for lts releases.
- **REQ-ECO.REL.PROC.GATES-007** The tool shall run without network access, read only the files listed in `gates.toml`, and write `release-gates.json` as RFC 8785 canonical JSON that includes the SHA-256 of every input file.
- **REQ-ECO.REL.PROC.GATES-008** A `--pre-tag` mode shall evaluate the gates for a proposed version before it is tagged, so the threat-review gate can be met before tagging as `QUAL.THREAT` requires.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/gates/all-pass/` (every gate file passing, release `v1.4.0-beta.1`), when `cargo xtask rel gate --release v1.4.0-beta.1 --channel beta --inputs fixtures/rel/gates/all-pass --json` runs, then it exits 0 and `release-gates.json` has `verdict: "pass"` and all venues enabled.
- **AC-2** (error path) Given `redteam-block/` (verdict file with `verdict: "block"`, task `inj-007`), then it exits 1 and the verdict names `agent-redteam` and `inj-007`; given `redteam-other-rc/` (verdict for a different candidate), it exits 1 with reason `candidate_mismatch`.
- **AC-3** (error path) Given `evid-missing-member/` (completeness file listing `vex.json` as missing), then it exits 1 naming `qual-evid` and `vex.json`.
- **AC-4** (error path) Given `threat-no-review/` and release `v1.5.0-beta.1` with no review record for `1.5`, then it exits 1 naming `qual-threat`; given release `v1.5.1` with no record, the gate is `not_applicable`.
- **AC-5** Given `cra-draft/` (classification record `draft`) and venues `gh-releases` (eu_distribution true) and `internal-mirror` (false), then it exits 0 with `verdict: "pass"`, `disabled_venues: ["gh-releases"]`.
- **AC-6** (determinism) Given the same inputs twice in different directory listing orders, then the two `release-gates.json` files are byte-identical.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel gate --release <tag> --channel <c> --inputs <dir> [--pre-tag] [--out <dir>] [--json]`. Writes one file into a local output root (W1).
- **Data structures:** `gates.toml` `[[gate]] id, producer, path, applies = {kinds = [first-minor|patch|any], channels = [...]}, missing_blocks, adapter`. Initial gate IDs and default input paths (relative to `--inputs`): `qual-evid` → `evidence/completeness.json`; `qual-threat` → `threat/reviews/<MAJOR.MINOR>.toml`; `qual-cra` → `cra/status.json`; `agent-redteam` → `redteam/verdict.json`; `qual-sbom` → `sbom/vuln-gate.json`; `plat-matrix` → `platform/gate.json`; `pkg-plan` → `packages/gate.json`; `os-sign` → `signing/os-readiness.json`; `flow-ga-<app>` → `acceptance/<app>-gate.json`; `i18n` → `i18n/gate.json`; `krn-xart` → `kernel/xart-report.json`; `lts` → `lts/gate.json`. `GateResult { id, status, reasons, input_sha256 }`; `Verdict { release, channel, verdict, gates, disabled_venues }`.
- **Algorithm:** load config; for each gate decide applicability from the parsed version and channel; read and adapt its file; fold to verdict (any block → block); apply the CRA venue rule; write canonical JSON.
- **Errors:** `GATE-INPUT-PARSE(gate)` counts as block for that gate; `CONFIG-PARSE` exits 2.
- **Determinism:** D0.
- **Performance budget:** under 2 s.
- **Crate:** `fid-xtask` (module `rel::gates`).

## Test fixtures

`fixtures/rel/gates/` with one synthetic file per gate per scenario (all-pass, redteam-block, redteam-other-rc, evid-missing-member, threat-no-review, cra-draft); CC-BY-4.0. Adapter shapes follow the producer nodes' published schemas; until those exist the fixtures define the minimal fields read (documented in `gates.toml` comments).

## Build prompt

> Add module `rel::gates` to the `fid-xtask` crate: `rel gate --release <tag> --channel nightly|beta|stable|lts --inputs <dir> [--pre-tag] [--out <dir>] [--json]`. Read `release/gates.toml` (`[[gate]] id, producer, path, applies.kinds (first-minor|patch|any), applies.channels, missing_blocks, adapter`). Gates: qual-evid (block if completeness file lists missing or mismatched members), qual-threat (for first release of a MAJOR.MINOR, block unless `threat/reviews/<MAJOR.MINOR>.toml` exists), qual-cra (if support-period or classification status is not `counsel_reviewed`, disable all venues in `release/venues.toml` with `eu_distribution = true`, do not block), agent-redteam (block if file missing, candidate differs, or verdict is `block`; copy task, agent and journal entry IDs into reasons), qual-sbom, plat-matrix (PASS/BLOCK), pkg-plan, os-sign, flow-ga-<app> for each app labelled ga, i18n, krn-xart (patch releases), lts (lts channel). A missing file blocks when `missing_blocks` is true; an unparsable file blocks its gate. Output `release-gates.json` in RFC 8785 canonical JSON with release, channel, verdict pass|block, per-gate status and reasons, SHA-256 of each input, disabled venues. Exit 0 on pass, 1 on block, 2 on config errors. No network, no clock. Tests: AC-1..AC-6 of spec node ECO.REL.PROC.GATES. Constraints: no panics, deny unwrap/expect, bounded parsers, BTreeMap only.

## Open questions

- The input file names above are proposals; `QUAL.EVID`, `QUAL.THREAT`, `QUAL.CRA`, `QUAL.SBOM`, `AGENT.REDTEAM`, `FLOW.RUNNER` and `I18N.XLATE` are not yet expanded and should adopt them or update `gates.toml` (cross-domain).
- REQ-004 reads QUAL.CRA-002 ("refuse to enable any EU distribution channel") as disabling EU venues only. If every public venue is reachable from the EU, this blocks all public publication until counsel review; that is intended [unverified legal reading].

## Children

_Leaf: none._
