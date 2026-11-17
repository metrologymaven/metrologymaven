---
id: ECO.REL.LTS
title: Long-term-support channel
level: 3
parent: ECO.REL
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-009]
covers: []
depends_on: [ECO.REL.PROC.VERSION]
uses_artifacts: [KRN.MAN.XART, KRN.MAN.MANI, ECO.REL.PROC.CHANNEL, ECO.REL.PROC.GATES]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.LTS: Long-term-support channel

## Purpose

Factories and regulated users can't take a new feature release every few weeks. An LTS channel gives them a fixed version with backported security and correctness fixes for a published window, while keeping kernel results bit-identical across patch releases (patch semver rule, CONSTITUTION §4). This leaf owns the LTS policy text, the machine-readable list of LTS lines with their support windows, and the gate that every LTS patch release must pass.

## Scope

**In:**
- `release/LTS.md`: policy (cadence, support window, what may be backported, what may not)
- `release/lts.toml`: one row per LTS line (`MAJOR.MINOR`), its branch, start date, end-of-support date and status
- `cargo xtask rel lts-check` (validates `lts.toml`) and `cargo xtask rel lts-gate` (gate for an LTS patch tag, writes `lts/gate.json` consumed by `ECO.REL.PROC.GATES`)
- Backport rules: label, link to the original change, and the kernel bit-identity rule

**Out:**
- Commercial terms, pricing and contracts (business documents outside the tree)
- The lts channel feed (owned by `ECO.REL.PROC.CHANNEL`, which reads `lts.toml`)
- The kernel bit-identity measurement itself (owned by `KRN.MAN.XART`)
- Evidence bundles for LTS releases (owned by `QUAL.EVID`; its gate applies to the lts channel like any other)

## Requirements

- **REQ-ECO.REL.LTS-001** `lts.toml` shall list each LTS line with `line` (`MAJOR.MINOR`), `branch`, `start` and `end` dates and `status` (`supported` or `ended`), and the check shall fail if `end` is earlier than `start` plus the minimum window stated in `LTS.md` (initially 24 months [unverified: window to be set by maintainers]).
- **REQ-ECO.REL.LTS-002** If an lts-channel tag's `MAJOR.MINOR` is not a line with status `supported`, or the tag's date is after the line's `end`, then the gate shall block with `LTS-NOT-SUPPORTED`.
- **REQ-ECO.REL.LTS-003** If any PR merged into the LTS branch since the previous patch tag of that line lacks exactly one of the labels `backport:security` or `backport:correctness`, then the gate shall block and name the PR.
- **REQ-ECO.REL.LTS-004** If a backport PR names no original change merged to the main branch and does not carry `lts-only` with a written reason, then the gate shall block and name the PR.
- **REQ-ECO.REL.LTS-005** If the `KRN.MAN.XART` patch comparison for the tag is missing or reports any differing digest against the previous patch of the line, then the gate shall block with `LTS-KERNEL-BITS`.
- **REQ-ECO.REL.LTS-006** If any algorithm implementation digest in the kernel manifest differs from the previous patch of the line, then the gate shall block with `LTS-KERNEL-CHANGED` naming each algorithm.
- **REQ-ECO.REL.LTS-007** The gate output shall list every backport with its PR number, label and original change, for inclusion in the release notes.
- **REQ-ECO.REL.LTS-008** `LTS.md` shall state that a correctness fix which changes kernel output cannot ship as an LTS patch, and shall state the alternative: an advisory on the LTS line and the fix in the next LTS line.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rel/lts/ok/` (line 1.2 supported until 2028-06-30, tag `v1.2.3` dated 2027-01-10, two PRs labelled `backport:security` and `backport:correctness` with original PR links, XART report all equal, manifests with equal implementation digests), when `cargo xtask rel lts-gate --tag v1.2.3 --inputs ok/ --out out/` runs, then it exits 0 and `out/lts/gate.json` has `verdict: pass` and two backports.
- **AC-2** (error path) Given `ended/` (line 1.0 status `ended`), then the gate blocks with `LTS-NOT-SUPPORTED`.
- **AC-3** (error path) Given `feature-pr/` (one PR labelled `feature`), then the gate blocks naming the PR.
- **AC-4** (error path) Given `kernel-bits/` (XART report with one differing digest), then it blocks with `LTS-KERNEL-BITS`; given `kernel-changed/` (one algorithm implementation digest changed), it blocks with `LTS-KERNEL-CHANGED` naming the algorithm.
- **AC-5** (error path) Given `lts.toml` with a line whose window is 12 months, when `rel lts-check` runs, then it exits 1 with `LTS-WINDOW`.
- **AC-6** (determinism) Running AC-1 twice gives byte-identical `gate.json`.

## Design

- **Registry command(s):** none; repository tools `cargo xtask rel lts-check --file release/lts.toml` (R0) and `cargo xtask rel lts-gate --tag <t> --inputs <dir> --out <dir>` (W1, writes the gate file).
- **Data structures:** `lts.toml` `[[line]] line, branch, start, end, status`; `[policy] min_window_months = 24`. Gate inputs: `prs.json` (merged PRs on the LTS branch since the previous patch, with labels and original-change links), `kernel/xart-report.json`, `kernel/manifest.json` and `kernel/manifest-previous.json`. Output `LtsGate { tag, line, verdict, reasons, backports: [{pr, label, original}] }`.
- **Algorithm:** parse tag (VERSION rules), look up the line, apply REQ-002 to REQ-006 in order, collecting all reasons, write canonical JSON.
- **Errors:** `LTS-NOT-SUPPORTED`, `LTS-LABEL(pr)`, `LTS-NO-ORIGIN(pr)`, `LTS-KERNEL-BITS`, `LTS-KERNEL-CHANGED(alg)`, `LTS-WINDOW(line)`, `INPUT-PARSE` (exit 2).
- **Determinism:** D0 (dates come from inputs, never the clock).
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `rel::lts`); policy in `release/LTS.md`.

## Test fixtures

`fixtures/rel/lts/` (ok, ended, feature-pr, kernel-bits, kernel-changed, short-window) with synthetic PR exports, XART reports and manifests; CC-BY-4.0.

## Build prompt

> In the implementation repository, write `release/LTS.md` (LTS cadence, minimum 24-month support window, only security and correctness backports, kernel results bit-identical across patches of a line, a kernel-output-changing correctness fix ships as an advisory plus a fix in the next LTS line) and `release/lts.toml` (`[policy] min_window_months`, `[[line]] line, branch, start, end, status`). Add module `rel::lts` to `fid-xtask`: `rel lts-check --file release/lts.toml` (LTS-WINDOW when end < start + window) and `rel lts-gate --tag vX.Y.Z --inputs <dir> --out <dir>` which blocks on: line not supported or tag after end (LTS-NOT-SUPPORTED); any merged PR without exactly one of backport:security or backport:correctness (LTS-LABEL); a backport with no original main-branch change unless labelled lts-only with a reason (LTS-NO-ORIGIN); missing or differing KRN.MAN.XART patch comparison (LTS-KERNEL-BITS); any changed algorithm implementation digest between the two kernel manifests (LTS-KERNEL-CHANGED). Write `lts/gate.json` in RFC 8785 canonical JSON with verdict, reasons and the backport list. No clock reads. Tests: AC-1..AC-6 of spec node ECO.REL.LTS. Constraints: no panics, deny unwrap/expect, bounded parsing, BTreeMap only.

## Open questions

- Support-window length, LTS cadence (for example one line per year) and how many lines are supported at once are maintainer calls; 24 months is a placeholder [unverified].
- QUAL.CRA-001 asks for a support period per product line; `lts.toml` is the natural data source for it, and QUAL.CRA should read it as an artefact.
- Non-kernel correctness fixes (for example an importer bug) may change outputs of non-kernel crates; they are allowed in LTS patches because the bit-identity rule applies to kernel results only.

## Children

_Leaf: none._
