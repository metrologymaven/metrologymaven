# Metrology Maven Opaque-Box E2E Test Suite

Requirement-driven, opaque-box end-to-end tests across **Tiers 1–4** and **Features F1–F7**.

## Quick Start

```bash
# Local run (auto mode: skips only host-C/SQLite-blocked or OS-specific checks)
python e2e/run_e2e.py

# CI / strict run (auto-enabled when CI=true; fails if any OS-supported capability is missing)
python e2e/run_e2e.py --strict --out /tmp/e2e-report.json

# Filter by tier or feature
python e2e/run_e2e.py --tier 1 --feature F2
python e2e/run_e2e.py --tier 4 --keep-workdirs
```

## CLI Options

| Flag | Values | Default | Description |
|---|---|---|---|
| `--mode` | `auto` \| `strict` | `strict` if `CI=true` else `auto` | Controls whether host C-toolchain skips fail the run |
| `--tier` | `1` \| `2` \| `3` \| `4` \| `all` | `all` | Run a single tier or all tiers |
| `--feature` | `F1`..`F7` | all | Run tests for one feature area |
| `--out` | `<path.json>` | `<tmp>/metrologymaven-e2e-report.json` | Output path for `e2e-report/v1` JSON |
| `--keep-workdirs` | flag | off | Keep per-test temporary sandboxes on disk |

## Exit Codes

- `0`: All executed tests passed (and in `--strict`, zero OS-supported `skipped_capability` tests).
- `1`: One or more tests failed (or an OS-supported test was skipped in `--strict` mode).
- `2`: Invalid runner CLI arguments.

## Feature Areas (F1–F7)

- **F1**: `fid` CLI, Canon, Ctrl & Bus Dispatch (`AUTO.CLI.*`, `CORE.JRN.CANON`, `AUTO.CTRL.CLIENT`, `RDL.TRACIM`)
- **F2**: `fid-slice` Vertical Slice 0 & 2D PDF Auto-Ballooning (`SLICE.*`, `RPT.BALLOON.AUTO.*`)
- **F3**: `fid-verify` & Result Package / Trust Verification (`TRUST.VERIFY.CLI`, `TRUST.PKG.*`, `TRUST.SIGN.*`, `TRUST.CHAIN.*`)
- **F4**: `fid-mcp` JSON-RPC 2.0 Stdio Session, Projection & Progressive Disclosure (`AUTO.MCP.*`)
- **F5**: Outreach, Static Site, Field Guide WASM & Release Verify Guide (`OUTR.DOCS.*`, `site/build.py`, `verify_guide.py`)
- **F6**: Spec Tooling, RDL/Tracim, `xtask` Gates, Scan & Release Flows (`scope.py`, `grow.*`, `tracim_public.py`, `xtask`, `SCAN.CMP.DEV.PRIM`, `ECO.REL.PROC.FLOW`)
- **F7**: Docs, Templates, Spec Drift & `generated/` Consistency (`check_spec_drift.py`, `docs/`, `README.md`, `.github/`, `release/`, `generated/`)

## Coverage Summary

- **Tier 1 (42 tests)**: 6 happy-path tests per feature area (`F1`–`F7`)
- **Tier 2 (40 tests)**: boundary, negative, and error-handling tests across `F1`–`F7` (`>=5` per feature)
- **Tier 3 (23 tests)**: pairwise cross-feature combinations covering all `21/21` `C(7,2)` `F1`–`F7` pairs
- **Tier 4 (7 tests)**: end-to-end real-world application scenarios (`T4-S1`..`T4-S7`)
- **Total**: **112** opaque-box E2E tests

## Integrity & Dual-Environment Rules

- Unimplemented target contracts (such as M2/M3/M4/M5 deliverables on pre-integration branches) **fail cleanly** instead of skipping.
- Only genuinely host-blocked C/SQLite crates (`fid-xtask`, `fid-val`, `fid-crash`) or OS-specific checks (`verify-linux` vs `verify-windows`) emit `skipped_capability` in `--mode auto`.
- Tests always use `sys.executable` and isolated temporary directories so tracked repository files are never mutated.
