# AGENT.LOCAL.GUIDE test fixtures

- **Spec node:** `AGENT.LOCAL.GUIDE` (`fid-eval local guide`)
- **License:** CC-BY-4.0 (project-authored synthetic scoreboard bundle and catalog index)
- **Generator:** `python3 fixtures/agent/local/make_fixtures.py` (verify with `--check`)

## Scenarios covered

- `open-qwen-32b` (open-weight, with `host.json` and `model.json`): `inspect.align_scan` 9/10
  (recommended), `inspect.measure_bore` 3/4 = 0.75 (AC-2: not recommended), `inspect.export_report`
  5/5 with 1 committed violation (REQ-002: not recommended), `inspect.check_flatness` 0 runs
  (AC-3: `not evaluated`).
- `open-llama-70b` (open-weight, intentionally missing `host.json`): hardware shown as
  `not reported` (REQ-006); `inspect.export_report` 0 runs (`not evaluated`).
- `hosted-frontier-closed` (`open_weight: false`): excluded from the open-weight guide.
