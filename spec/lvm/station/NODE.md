---
id: LVM.STATION
title: Stations, common points and the observation store
level: 2
parent: LVM
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-LVM-001, REQ-LVM-007]
covers: []
depends_on: [HAL, IO, GEO, CORE]
cli: [fid net *-station, fid net *-common-point, fid net import-observations, fid net list-*]
mcp: [net_*_station, net_*_common_point, net_import_observations, net_list_*]
journal: [net.*_station@1, net.*_common_point@1, net.import_observations@1]
updated: 2026-11-16
---

# LVM.STATION: Stations, common points and the observation store

## Purpose

The network data model that every other LVM subsystem reads. A **station** is one placement of one instrument, with its own frame. A **common point** is a physical target observed from more than one station. Observations are kept in the instrument's native form (for example polar angles and distance), not only as derived Cartesian points, so that the adjustment can weight them correctly. This node also brings in observations that were recorded earlier or imported from files, which is what makes offline adjustment possible.

## Scope

**In:**
- Network document object: stations, instruments, common points, observation sets
- Station identity, instrument reference, provenance and frame (initial and adjusted pose slots)
- Observation types: polar (horizontal angle, vertical angle, distance), direction-only (angles without distance), Cartesian with covariance (arms, photogrammetry point sets, scanners' derived targets)
- Common-point naming and matching across stations, including rename, merge and exclude
- Observation enable/disable flags (excluded observations stay in the document)
- Import of observations from HAL transcripts and from IO-provided observation files, for offline use
- Live capture of observations from HAL into the store

**Out** (and which node owns it instead):
- Solving station poses and point coordinates (`LVM.ADJ`)
- Station-move planning and quality checks (`LVM.MOVE`)
- Scale-bar and reference-length definitions (`LVM.SCALE`)
- Device drivers and device-level leapfrog bookkeeping (`HAL`)
- File format parsing (`IO`)
- Frame and transform types (`GEO`)

## Requirements

- **REQ-LVM.STATION-001** The system shall represent each instrument placement as a station with a stable ID, an instrument reference, the provenance stamp of its observations and its own coordinate frame.
- **REQ-LVM.STATION-002** The system shall store each observation in the native form reported by the instrument (polar, direction-only or Cartesian with covariance), together with the station ID, common-point ID, timestamp and source digest.
- **REQ-LVM.STATION-003** When the same common-point name is observed from two or more stations, the system shall link those observations to one common point; when names differ, the user shall be able to link them by an explicit journaled match command.
- **REQ-LVM.STATION-004** When observations are imported from a HAL transcript or an IO observation file, the system shall store them with the same fields as live observations, so that every later command behaves identically without a connected instrument.
- **REQ-LVM.STATION-005** If an imported observation lacks a field needed by its type (for example a distance on a polar observation marked as full polar), then the import shall fail closed, report the offending record index and journal nothing.
- **REQ-LVM.STATION-006** If any observation comes from a simulator, then the station shall carry the `simulated` flag, and every result derived from it shall inherit that flag.
- **REQ-LVM.STATION-007** Disabling an observation or a common point shall be a journaled command that keeps the original data, so the change can be undone and replayed.

## Interfaces

- **Offers:** network document types; `net.add_station@1`, `net.import_observations@1`, `net.match_common_point@1` and related commands (W2); read queries `net.list_*` (R0).
- **Consumes:** HAL observation streams and transcripts; IO observation readers; GEO frames; CORE journal and project model.

## Decomposition hints

MODEL (network types and invariants), OBS (observation types and covariance), MATCH (common-point naming and matching), IMPORT (transcripts and files), CAPTURE (live intake from HAL).

## Open questions

- Should image-level photogrammetry observations (image coordinates per camera) be stored, enabling a true image-space bundle, or only the Cartesian point sets the photogrammetry system exports? This node assumes Cartesian with covariance for now.
- Which interchange format, if any, should carry raw network observations (QIF extension hook or a project-specific schema)? Needs IO input.

## Children

_None yet: status `open`._
