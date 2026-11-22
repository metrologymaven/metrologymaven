# QUAL.IQOQ.GOLDEN test fixtures

- **Spec node:** `QUAL.IQOQ.GOLDEN` (`validate.register_golden@1`)
- **License:** CC-BY-4.0 (synthetic datum-plate inspection project)
- **Generator:** `python3 fixtures/qual/iqoq/golden/make_fixtures.py` (verify with `--check`)

## Files

- `plate.fid`: synthetic project journal on branch `main` (`seq` 1..=50) containing 6 committed
  `ResultRecord` entries inside range `10..=42` (`seq` 12, 18, 23, 29, 35, 41) and non-result
  entries in `1..=9` and `43..=50`.
- `expected.json`: independently computed SHA-256 of the RFC 8785 canonical JSON array of the
  6 `ResultRecord` digests in `main:10..=42`.
