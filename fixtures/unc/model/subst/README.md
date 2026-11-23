# UNC.MODEL.SUBST.INPUT test fixtures

- **Spec node:** `UNC.MODEL.SUBST.INPUT` (ISO 15530-3 calibrated-workpiece inputs & checks)
- **License:** CC-BY-4.0 (synthetic ring-gauge and bore datasets in SI units)
- **Generator:** `python3 fixtures/unc/model/subst/make_fixtures.py` (verify with `--check`)

## Files

- `ring_gauge_ok.json` (AC-1): calibrated ring `50.0012 mm` (`0.0500012` m), `U = 0.6 um` (`6.0e-7` m),
  `k = 2`, DCC source, 20 runs of 3 repetitions on the calibrated workpiece and 5 production runs,
  all under identical `Conditions`.
- `bore_ok.json`: calibrated bore `25.0005 mm` (`0.0250005` m), `U = 0.8 um` (`8.0e-7` m), `k = 2`,
  certificate source, 20 runs of 3 repetitions.
- `ring_gauge_insufficient_runs.json` (AC-2): 5 calibrated runs where default profile requires 20.
- `ring_gauge_strategy_mismatch.json` (AC-3): production measurements with a different `strategy_digest`.
- `ring_gauge_missing_k.json` (AC-4): missing coverage factor `k` in calibrated reference.
