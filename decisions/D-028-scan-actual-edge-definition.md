---
id: D-028
title: Definition of the actual edge for trim-line deviation
status: open
raised_by: SCAN.EDGE split
blocks: [SCAN.EDGE.EST]
decide_by: before SCAN.EDGE.EST is expanded
updated: 2026-11-16
---

# D-028: Definition of the actual edge for trim-line deviation

## Question

On scan data of a trimmed or cut edge that is rounded, chamfered or burred, which point on the measured surface counts as "the actual edge" whose trim and normal deviation SCAN.EDGE reports?

## Context

SCAN.EDGE reports in-surface (trim) and normal deviation of the actual part boundary against the nominal CAD edge (GOM.SRF.06, PW.CMP.04). For a clean free boundary of a mesh the answer is the mesh boundary. For real sheet-metal edges the scan usually wraps around the edge: the surface turns through a radius or a burr, and the scanner may or may not capture the edge face. Different reasonable definitions give answers that differ by up to the edge radius or sheet thickness [unverified], so the choice changes reported values and must be stated with every result.

The choice needs input from sheet-metal and body-in-white users (and possibly OEM measurement guidelines), so it is not an agent's to make. The rest of SCAN.EDGE (nominal stations and station deviation from a given actual boundary) is independent of the choice and proceeds as leaves.

## Options

| Option | For | Against |
|---|---|---|
| A. Tangency break: the edge is where the measured surface normal departs from the nominal surface normal by more than a threshold angle | Simple; works with one-sided scans | Threshold-dependent; burrs move it |
| B. Curvature threshold: the edge is the first point along the section where measured curvature exceeds a threshold | Robust to small noise with smoothing | Two parameters; sensitive to point density |
| C. Projected silhouette: the outermost measured point in the nominal tangent plane, perpendicular to the nominal edge | Matches a gauge or caliper view; no thresholds | Includes burrs; needs wrap-around data |
| D. Offer A and C as named methods; the method and parameters are recorded with every result and the default is set per defaults profile | Covers both user groups; honest provenance | More to test and document |

## Recommendation

D, with C as default for free edges of meshes and A for one-sided sheet scans [unverified]; confirm with at least two sheet-metal users before the leaf is written.

## Decision

_Open._ When decided: the option, who decided, the date, and any follow-up nodes to update (SCAN.EDGE.EST, possibly IND.SHEET).
