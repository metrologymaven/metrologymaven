---
id: PLAT.XHOST.SDKTAB
title: SDK platform table and catalog cross-check
level: 3
parent: PLAT.XHOST
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.XHOST-001]
covers: []
depends_on: [PLAT.MATRIX.FILE, HAL]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.XHOST.SDKTAB: SDK platform table

## Purpose

Publishes, for every catalog device integrated through a vendor SDK, the operating systems and architectures that SDK supports and therefore where its driver host must run. The table (`platform/sdk-platforms.toml`) is cross-checked against the device catalog so no SDK device is missing, and against the support matrix so every host platform is one the project actually supports.

## Scope

**In:**
- Table schema: device ID, SDK OS list, SDK architecture list, required host platform, source reference, verification status
- Validator with catalog and matrix cross-checks
- Initial rows for the catalog's SDK devices, marked unverified

**Out** (and which node owns it instead):
- The device catalog and its machine-readable export (`HAL`)
- Driver hosts, protocol and packaging (`HAL`, `PLAT.XHOST.PKG`)
- Showing the table to users through `fid doctor` (`PLAT.DOCTOR.HOSTCHK`) and the docs (`OUTR.DOCS`)

## Requirements

- **REQ-PLAT.XHOST.SDKTAB-001** The table schema shall define rows with `device` (catalog ID), `sdk_os` (list), `sdk_arch` (list), `host` (one OS and architecture where the project runs the driver host), `reference` (URL or document name) and `verified` (bool).
- **REQ-PLAT.XHOST.SDKTAB-002** If a catalog device whose integration path is `SDK` has no row, then validation shall fail naming the device.
- **REQ-PLAT.XHOST.SDKTAB-003** If a row's `host` is not in `sdk_os` and `sdk_arch`, or is not an effective Tier 1 or Tier 2 combination in the support matrix at the device's phase, then validation shall fail naming the row.
- **REQ-PLAT.XHOST.SDKTAB-004** If a row names a device that is not in the catalog or whose path is not `SDK`, then validation shall fail naming the row.
- **REQ-PLAT.XHOST.SDKTAB-005** The validator shall report the count of rows with `verified = false`, and the docs page generated from the table shall mark each such row as unverified.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/xhost/catalog-excerpt.toml` (3 SDK devices, 1 IPP device), `fixtures/plat/xhost/sdk-valid.toml` (3 rows, 2 unverified) and `fixtures/plat/matrix/valid.toml`, when `cargo xtask plat-xhost validate --phase P3` runs, then it exits 0 and prints `3 rows, 2 unverified`.
- **AC-2** (error path) Given `fixtures/plat/xhost/sdk-missing.toml` with one SDK device absent, when validated, then it exits 1 with `MissingRow { device }`.
- **AC-3** (error path) Given `fixtures/plat/xhost/sdk-bad-host.toml` whose host `macos/aarch64` is not in its `sdk_os`, when validated, then it exits 1 with `HostNotSupportedBySdk`.
- **AC-4** (error path) Given a row for the IPP device, when validated, then it exits 1 with `NotSdkDevice { device }`.
- **AC-5** Given the shipped table and the HAL catalog export, when validated in CI, then it exits 0.

## Design

- **Registry command(s):** none; internal data and `xtask plat-xhost validate`.
- **Data structures:** `SdkTable { schema_version, rows: Vec<SdkRow> }`; `SdkRow { device: DeviceId, sdk_os: Vec<OsFamily>, sdk_arch: Vec<Arch>, host: (OsFamily, Arch), reference: String, verified: bool }`. Catalog input: a list of `{ id, path, phase }` read from HAL's catalog export (fixture format until HAL defines it).
- **Algorithm:** parse with size cap (1 MiB); index catalog by ID in a `BTreeMap`; check each rule; matrix lookup via `fid-platform::matrix::lookup` using the host OS's lowest listed Tier 1/2 version.
- **Errors:** `TooLarge`, `Parse`, `MissingRow`, `UnknownDevice`, `NotSdkDevice`, `HostNotSupportedBySdk`, `HostNotSupportedByMatrix`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `sdk_table`).

## Test fixtures

Hand-written TOML under `fixtures/plat/xhost/` using invented device IDs (`DEV.TST.01` etc.), plus the shared matrix fixture. Licensed with the code. Real rows in the shipped table cite vendor documentation and stay `verified = false` until a maintainer checks them.

## Build prompt

> In crate `fid-platform`, add module `sdk_table` and `xtask plat-xhost validate [--table <file>] [--catalog <file>] [--matrix <file>] [--phase <P>]`. Table `platform/sdk-platforms.toml` (schema_version 1, cap 1 MiB) has rows `device`, `sdk_os` (list), `sdk_arch` (list), `host` = { os, arch }, `reference`, `verified`. Validate: every catalog device with path `SDK` has a row; every row names an SDK device in the catalog; `host` is within the row's SDK OS and arch lists and is an effective Tier 1 or Tier 2 combination in the support matrix (`fid-platform::matrix`) at the device's phase. Print row count and unverified count.
>
> Ship the initial table with rows for the catalog's SDK devices, each `verified = false` with the vendor reference. Acceptance tests: the five criteria above with fixtures in `fixtures/plat/xhost/`. Constraints: CONSTITUTION §8; never copy vendor SDK text, only cite. Commit: `feat(fid-platform): SDK platform table`.

## Open questions

- HAL has not yet specified a machine-readable catalog export; until it does, the validator reads a minimal `{ id, path, phase }` list generated from `seeds/devices.md`.

## Children

_Leaf: none._
