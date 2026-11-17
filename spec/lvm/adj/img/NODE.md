---
id: LVM.ADJ.IMG
title: Image-level photogrammetry bundle extension
level: 3
parent: LVM.ADJ
kind: module
status: open
phase: later
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM.ADJ-001, REQ-LVM.ADJ-002]
covers: []
depends_on: [LVM.ADJ.SOLVE, LVM.STATION.OBS]
cli: [fid net *-camera]
mcp: [net_*_camera]
journal: [net.*_camera@1]
updated: 2026-11-16
---

# LVM.ADJ.IMG: Image-level photogrammetry bundle extension

## Purpose

A later extension (assigned to `LVM.ADJ` at the level-2 review) that lets photogrammetry enter the network as image coordinates per camera exposure instead of only as Cartesian point sets. It adds an image observation type, camera models and collinearity equations to the unified adjustment, so a photogrammetry bundle can be solved together with trackers and other instruments.

## Scope

**In:**
- Image observation payload (exposure ID, target ID, image x and y with 2×2 covariance) as a new variant of the `LVM.STATION.OBS` payload set
- Camera definitions: principal distance, principal point, lens distortion parameters, each fixed or self-calibrated
- Exposures as stations with a camera reference
- Collinearity observation equations and Jacobians
- Weighting of image observations from UNC camera models or declared values

**Out** (and which node owns it instead):
- Image processing and target detection (photogrammetry system or `HAL`)
- Solver core (`LVM.ADJ.SOLVE`); this node adds equations and parameters to it
- Cartesian point sets from photogrammetry systems (`LVM.STATION.OBS`, P3)

## Requirements

- **REQ-LVM.ADJ.IMG-001** The system shall store image observations as image coordinates in metres on the image plane with a 2×2 covariance, linked to an exposure and a common point.
- **REQ-LVM.ADJ.IMG-002** When a network contains image observations, the adjustment shall include their collinearity equations in the same unified solution as all other observation types.
- **REQ-LVM.ADJ.IMG-003** The system shall let each camera parameter be fixed or estimated, and shall report estimated camera parameters with covariance.
- **REQ-LVM.ADJ.IMG-004** Given a noise-free synthetic image bundle, the adjusted coordinates shall match truth within 1e-9 m [unverified].

## Interfaces

- **Offers:** camera commands `net.*_camera@1` (W2); image observation equations for `LVM.ADJ.SOLVE`.
- **Consumes:** `LVM.STATION.OBS` payload extension point; `LVM.ADJ.SOLVE`; UNC camera models (not yet defined in UNC).

## Decomposition hints

OBS (image payload), CAM (camera models and distortion), COLL (collinearity equations), CALIB (self-calibration controls).

## Open questions

- Phase is `later` until the maintainers decide otherwise.
- Which distortion model set is the default (for example a Brown-type radial and decentring model) [unverified]?
- UNC has no camera uncertainty model yet; a cross-domain request to UNC is needed before this node is expanded.

## Children

_None yet: status `open`._
