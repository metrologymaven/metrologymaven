# `fixtures/unc/dec/risk/`

Test fixtures for spec node `UNC.DEC.RISK` (`fid-kdec::risk`, algorithm `dec.risk.compute@1.0.0`).

- `make_cases.py` — independent stdlib-Python oracle (`decimal` at 65 working digits) that generates and verifies the CSV files (`--check`).
- `specific.csv` — AC-1 test cases for specific consumer and producer risk (`1 - p` and `p` as exact `f64`).
- `global.csv` — AC-2 and AC-4 test cases with 50-digit reference global consumer and producer risks for two-sided, one-sided, positive/negative guard-band, shifted-prior, empty-zone, and tiny-`u` scenarios. No values are copied from JCGM 106 examples.

Licence: CC-BY-4.0.
