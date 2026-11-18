# Tiers fixture: no table under the heading

Hand-labelled (PLAT.MATRIX.POLICY.DEFS REQ-003). Expected: `NoTable` only. The pipe line
below has no delimiter row, so it is not a table, and the table after `## Other` is outside
the section.

## Tiers

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
Tier 1 builds and runs the full test suite.

## Other

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | a | b | c | d |
| Tier 2 | a | b | c | d |
| Tier 3 | a | b | c | d |
