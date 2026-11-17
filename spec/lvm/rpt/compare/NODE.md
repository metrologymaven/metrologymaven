---
id: LVM.RPT.COMPARE
title: Comparison of two adjustments
level: 3
parent: LVM.RPT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.RPT-003]
covers: []
depends_on: [LVM.ADJ.SOLVE, LVM.ADJ.STATS]
cli: [fid net compare-adjustments]
mcp: [net_compare_adjustments]
journal: []
updated: 2026-11-16
---

# LVM.RPT.COMPARE: Comparison of two adjustments

## Purpose

Compares two adjustment results of the same network, for example before and after excluding a flagged observation or changing the datum, so the user can see what moved and why. It lists per-point coordinate and uncertainty changes and the differences in enabled observations, weights and datum.

## Scope

**In:**
- Pairing of points and stations between two results by stable ID
- Per-point coordinate change, change in uncertainty, per-station pose change
- Differences in enabled observations, constraints, weighting and datum
- Points or stations present in only one result

**Out** (and which node owns it instead):
- Producing the adjustments (`LVM.ADJ.SOLVE`)
- Report layout (`LVM.RPT.EXPORT`, `RPT`)

## Requirements

- **REQ-LVM.RPT.COMPARE-001** When `net.compare_adjustments@1` runs on two results of the same network, the system shall list per common point the change in adjusted coordinates and in standard uncertainty per axis.
- **REQ-LVM.RPT.COMPARE-002** The comparison shall list observations and constraints enabled in one result but not the other, and any difference in datum or weighting.
- **REQ-LVM.RPT.COMPARE-003** If the two results come from different networks, then the command shall fail with `NET_COMPARE_MISMATCH`.
- **REQ-LVM.RPT.COMPARE-004** If the two results use different datums, then coordinate changes shall be reported only after a stated common similarity alignment, or not at all, and the comparison shall say which.

## Interfaces

- **Offers:** `net.compare_adjustments@1` (R1); comparison block for `LVM.RPT.EXPORT`.
- **Consumes:** two `LVM.ADJ.SOLVE` results with `LVM.ADJ.STATS`.

## Decomposition hints

Likely a single leaf once result types are fixed.

## Open questions

- For different datums, is a similarity alignment (via ALN) acceptable, or should the comparison refuse? Needs a reviewer.

## Children

_None yet: status `open`._
