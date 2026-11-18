# Tiers fixture: empty cells

Hand-labelled (PLAT.MATRIX.POLICY.DEFS AC-3). Expected, in this order:
`EmptyCell { tier: tier2, column: Support response }`,
`EmptyCell { tier: tier3, column: Promotion/demotion }` (the row is one cell short).

A table before the heading is ignored:

| Tier | Note |
|---|---|
| Tier 1 | |

## Tiers

Text between the heading and the table is allowed.

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|:---|:---:|---|---|--:|
| **Tier 1** | Full test suite every release. | Yes. | Before release. | Two green release candidates. |
| Tier 2 | Build every release. | No. |   | Promotion rule. |
| tier3 | None. | No. | Community. |
