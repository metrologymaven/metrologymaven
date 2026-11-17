# launch-1 growth review

Date 2026-11-17. Branch `grow/launch-1`. 33 agent batches expanded the 349 open nodes in launch-1's scope. 13 of them were interrupted by a usage limit and resumed from their saved state.

## Result

| | Before | After |
|---|---|---|
| Nodes (whole tree) | 1,092 | **2,108** |
| Buildable leaves | 383 | **1,402** |
| launch-1 leaves | 322 | **1,336**, all ordered in **39 build waves** with no dependency loop |
| launch-1 nodes still open | 349 | **72**, mostly expert-authored content (DMT rows, ORS cases, MPE tables, SPC constants) and per-app screen mockups |
| Validator | 0 errors | **0 errors**; 4 size warnings on deliberately open nodes |
| Decisions blocking launch-1 | 0 | 0 |

## Fixes made in this review

- **Dependency loop:** HAL.HOST depended on all of ECO, which closed the loop CMM → ECO → HAL → LVM. It is narrowed to ECO.SDK.
- **MIG:** its domain-level dependency no longer names AUTO (L3) or CMM (outside launch-1).
- **Build ordering:** `scope.py milestone` now orders leaves by their own dependencies. Inheriting ancestors' whole-subsystem dependencies had created about 1,100 falsely unordered leaves.
- **launch-1 edits:**
  - I18N's string catalogue and locale pieces are in scope;
  - later-phase pieces (Probe and Network app shells, the J7 gap-and-flush job, regulated approval gates, the Python console, a scanner benchmark) are excluded;
  - correlation study #1 is reworded per D-021: it compares against public reference data only, with no comparisons against named incumbents.

## Decision opened

- **D-035:** constitution gaps (injected-input principal; filtering registry queries; bounded egress responses; new foundation crates; crate layers; Lean human assertion; consistency fixes). Recommended: accept all.

## Known remaining items (recorded in node Open questions)

- **Interface agreements between leaves** that agents assumed and that owners must confirm when implementing:
  - ResultRecord storage owner
  - `x-reason-code` and `suggested_fixes` envelope fields
  - per-step metadata in the Command Program format
  - ChartSeries labels
  - the `pick:*` schema keywords
- **Expert inputs**, marked `[unverified]` or left open:
  - DMT rows, ORS cases, MPE tables, SPC constants
  - the near-zero pass rule for uncertainty tests
  - gap-and-flush definitions
  - sheet-metal edge defaults
- **People and funding decisions:**
  - the blind-set custodian
  - TraCIM automation terms
  - NIST/PTB data licences
  - the code-signing certificate holder
  - who pays for agent-scoreboard model runs
  - counsel reviews named in D-013, D-015, D-021, D-024 and D-033
- **HAL.ACQ.SESS.LIFE** wires the P2 trust grade. launch-1 uses it with `declared`/`unknown` grades only.
