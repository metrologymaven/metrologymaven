# Tiers fixture: placeholder-only cells

Hand-labelled (PLAT.MATRIX.POLICY.DEFS REQ-003). Expected, in this order:
`EmptyCell { tier: tier1, column: CI obligation }` (`-`),
`EmptyCell { tier: tier2, column: Release-blocking }` (`**`),
`EmptyCell { tier: tier3, column: Support response }` (an em dash),
`EmptyCell { tier: tier3, column: Promotion/demotion }` (`` ` ` `` and `_`).

## Tiers

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | - | Yes. | Before release. | Two green release candidates. |
| Tier 2 | Build every release. | ** | Best effort. | Promotion rule. |
| Tier 3 | None. | No. | — | ` ` _ |
