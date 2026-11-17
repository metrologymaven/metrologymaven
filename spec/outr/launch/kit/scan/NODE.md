---
id: OUTR.LAUNCH.KIT.SCAN
title: Inspect and Report demo kit for launch-1 events
level: 4
parent: OUTR.LAUNCH.KIT
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH.KIT-001, REQ-OUTR.LAUNCH.KIT-002, REQ-OUTR.LAUNCH.KIT-003, REQ-OUTR.LAUNCH.KIT-005]
covers: []
depends_on: [OUTR.LAUNCH.KIT.CHECK]
uses_artifacts: [ECO.REL.PKG, ECO.DATA.EX, OUTR.CONTENT.TUT.INSPECT]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.KIT.SCAN: Inspect and Report demo kit for launch-1 events

## Purpose

The first demo kit: everything a volunteer needs to show the Inspect and Report apps at a conference or trade show with no network and no instruments. It holds the offline installers of the targeted release, an example project with a scan, CAD and drawing, a scripted demo that replays to `IDENTICAL`, a hardware list (a laptop and a screen), a one-page handout and a pre-event checklist. It shows only file-based workflows, so no device and no motion is involved.

## Scope

**In:** `kits/scan-inspect/` with `kit.toml`, installers for the PLAT Tier 1 desktop systems, the example project and journal, `DEMO.md` (scripted demo, about 10 minutes), `HARDWARE.md`, `handout.md`, `CHECKLIST.md`, the trade-dress review record.
**Out:** the kit check (`OUTR.LAUNCH.KIT.CHECK`); installers (`ECO.REL.PKG`); example datasets (`ECO.DATA.EX`); booth costs and sponsorship (business documents outside this tree); device demos (later kits).

## Requirements

- **REQ-OUTR.LAUNCH.KIT.SCAN-001** The kit's scripted demo shall follow acceptance job JOB.ACC.01 (scan to colour map and GD&T report) and show the resulting report in the Report app, in our own words.
- **REQ-OUTR.LAUNCH.KIT.SCAN-002** The kit manifest shall declare `devices = []` and a hardware list of one laptop at or above the minimum hardware profile (D-014) and one external display.
- **REQ-OUTR.LAUNCH.KIT.SCAN-003** `kit-check --run` shall pass on the kit with networking disabled for the release it targets.
- **REQ-OUTR.LAUNCH.KIT.SCAN-004** The pre-event checklist shall include: run `kit-check --run` on the event laptop, disable networking, confirm the installer checksums, and confirm the handout's review record is current.
- **REQ-OUTR.LAUNCH.KIT.SCAN-005** The handout shall state the app status labels exactly as the release's app status data gives them (alpha, beta or stable) and make no capability claim without a capability tag that is live in the release's parity report.

## Acceptance criteria

- **AC-1** Given `kits/scan-inspect/` for the targeted release, when `fid-outreach kit-check --kit kits/scan-inspect --rules claim-rules.toml --roster roster.txt --checklist checklist.toml --author <kit author> --run` runs in the networking-disabled CI job, then it exits 0.
- **AC-2** Given `kit.toml`, when a test reads it, then `devices` is empty and the hardware list has exactly the laptop and display entries.
- **AC-3** Given `handout.md` and the release's parity report, when `fid-outreach cap-check` runs on it, then it reports nothing.
- **AC-4** Given `CHECKLIST.md`, when a test reads it, then it contains the four items of REQ-004.

## Design

- **Registry command(s):** none; a content bundle checked by `kit-check` and `cap-check`.
- **Demo script:** numbered steps with expected screens; the journal of a rehearsal run is the kit's demo journal; the same example project as the Inspect tutorial where possible.
- **Determinism:** D1 (the replay outcome comes from the product binary).
- **Crate:** none (content).

## Test fixtures

The example project and journal from ECO's example-project set (public or CC-licensed scan, CAD and drawing); the targeted release's installers and checksums.

## Build prompt

> Assemble `kits/scan-inspect/`, a self-contained trade-show demo kit for the Inspect and Report apps of the targeted release. Include: the release's desktop installers for the Tier 1 systems; the example project and journal (reuse the Inspect tutorial's project if it fits); `DEMO.md`, a 10-minute scripted demo following acceptance job JOB.ACC.01 (import CAD with PMI and a scan, align, colour map, GD&T, report) and ending in the Report app, in your own words; `HARDWARE.md` (one laptop at or above the minimum hardware profile, one external display; no instruments); `handout.md` (one page; app status labels exactly as the release states them; tag every capability claim with `<!-- cap: <seed ID> -->`; no "certified", "compliant" or comparative claims); `CHECKLIST.md` (run `kit-check --run` on the event laptop, disable networking, confirm installer checksums, confirm the handout review record is current); a trade-dress review record by a reviewer other than you; and `kit.toml` with `devices = []` and SHA-256 for every file.
>
> Done when `fid-outreach kit-check --run` exits 0 in the networking-disabled CI job and `fid-outreach cap-check` reports nothing on the handout.

## Open questions

- JOB.ACC.05 (kiosk run) could be a second demo track once the kiosk layout exists; it is not required here.

## Children

_Leaf: none._
