# VIZ.SHELL.SPKTREE results: element tree with 10k items

Overall: **incomplete** (not yet measured)

No timings have been recorded. The spike was built without access to the PLAT H1 reference
machine or to a GPU, so every timed criterion is "not run" and no recommendation on D-005 is
made. To measure, on H1 (or the closest integrated-GPU machine, marked [unverified]):

```text
cargo run --release -p fid-spike-ui --example tree10k -- --bench out --hardware "<CPU, RAM, OS>" [--h1]
```

That writes `out/REPORT.md` (this layout, filled in) and `out/timings.csv` (raw per-frame
timings). Then do the manual AC-4 check with Windows Narrator or NVDA on five items, fill in
the screen-reader table, rerun with `--screen-reader pass|fail`, and replace this file with
the result.

## Criteria

| Criterion | Measured | Threshold | Result |
|---|---|---|---|
| AC-1 scroll, p95 frame time | - | < 33 ms | not run |
| AC-2 expand-all, p95 frame time | - | < 33 ms | not run |
| AC-2 collapse-all, p95 frame time | - | < 33 ms | not run |
| REQ-002 filter typing, p95 frame time | - | < 33 ms | not run |
| AC-3 keyboard sequence | 48 of 48 steps in CI (model only, no window) | all steps | pass in CI |
| AC-4 screen reader (manual) | see log | role, name, level, expanded for 5 items | not run |

## Known limitations of the prototype

- Frame time is CPU wall time from input to submit (vsync off); GPU completion is not awaited.
- The 10^6-point viewport is static geometry redrawn every frame.
- Only rows on screen (virtualised) reach AccessKit; position and set size use the full sibling list.
- No Tree container role is set; screen readers see tree items without a tree parent [unverified].
- egui default fonts are not built (licence gate in deny.toml); a system font is loaded at run time.
- No Metal backend (advisory gate on paste via metal); macOS is not measured.
