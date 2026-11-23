# UNC.PROP.VALID test fixtures

- **Spec node:** `UNC.PROP.VALID` (`unc.prop.validate_lpu@1.0.0`)
- **License:** CC-BY-4.0 (synthetic reference models and exact decimal tolerance tables)
- **Generator:** `python3 fixtures/unc/valid/make_fixtures.py` (stdlib Python 3 only; verify with `--check`)

## Files

- `linear_sum.json` (AC-1): `Y = X1 + X2` with `X1 ~ N(10.0, 0.3)` and `X2 ~ N(5.0, 0.4)`,
  200 000 MCM trials seeded with `0xA11CE`, compared against analytical LPU at `p = 0.95` and
  `n_dig = 2`. Expected outcome: `validated = true`.
- `square_near_zero.json` (AC-2): `Y = X^2` with `X ~ N(0.1, 1.0)` (mean near zero so
  first-order LPU has a strictly non-empty interval `U_p > 0` per REQ-UNC.PROP.VALID-003,
  while strong nonlinearity makes LPU disagree sharply with MCM), 200 000 MCM trials seeded
  with `0xA11CE` at `p = 0.95` and `n_dig = 2`. Expected outcome: `validated = false`.
- `delta_cases.csv` (AC-3): exact decimal tolerances `delta = 0.5 * 10^l` computed with
  Python `fractions.Fraction` across `u(y)` including `0.0123`, `1.0e-6`, `9.96e-7` and
  rounding-carry boundaries for `n_dig` in `{1, 2, 3}`.
