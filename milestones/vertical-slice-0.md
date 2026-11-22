---
id: vertical-slice-0
title: Vertical Slice 0 - playable end-to-end Inspect, 2D PDF Ballooning, Report and Verify
status: proposed
targets: [KRN.NUM, KRN.LSQ, KRN.ENV.TAN.PLANE, GDT.DRF.BUILD, GDT.ZONE.ZTYPE, UNC.DEC.TYPE, CORE.JRN.CANON, TRUST.PKG.MANIFEST, TRUST.PKG.READER, RPT.MODEL.FMT, RPT.BALLOON]
exclude: [CMM, LVM, IND, ENT, VIZ.WEB, I18N.XLATE]
updated: 2026-11-22
---

# Milestone vertical-slice-0: Playable end-to-end Inspect, 2D PDF Ballooning, Report and Verify

## Goal

Give real metrologists, machine shops and contributors a **runnable, end-to-end binary on Day 1** (`crates/fid-slice`) that exercises the real deterministic Rust crates across every layer—without waiting for all 1,400+ leaves of `launch-1`:

1. **Deterministic KNP-1 Fitting & 3-2-1 DRF (`fid-fit`, `fid-gps`)**: certified tangential datum planes (`A|B|C`), least-squares plane & cylinder fits, closed-form GD&T tolerance zones (`TwoPlanes`, `Cylinder`).
2. **Side-Preserving Formatting & Decision Rules (`fid-num::fmt`, `fid-kdec`)**: exact decimal specification limits and honest 4-valued conformance outcomes.
3. **Canonical Hash-Chained Journal & Verifiable Result Package (`fid-canon`, `fid-pkg`)**: RFC 8785 canonical JSON events, AS9102 Form 3 CSV export, deterministic `.zip` Result Package with in-toto statement, verified fail-closed by `PackageReader`.
4. **2D PDF Drawing Ballooning Wedge (`RPT.BALLOON.AUTO`)**: drop a 2D PDF drawing in, extract text + bounding boxes, propose balloons on a W1 draft branch for H2 adoption, and export a ballooned PDF + Form 3 starter CSV.

## Exit criteria

- `cargo test -p fid-slice` passes end-to-end, proving byte-identical (`D0`) Result Package digests across runs and fail-closed tamper detection.
- `cargo run -p fid-slice -- --demo --out <dir>` writes and verifies a complete Result Package + AS9102 Form 3 CSV with zero setup.
- All four `RPT.BALLOON.AUTO` leaves (`TEXT`, `PARSE`, `PROPOSE`, `EXPORT`) build and wire into `fid-slice` / `fid` for Day-1 2D PDF ballooning.
