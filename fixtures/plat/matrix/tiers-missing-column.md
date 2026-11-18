# Tiers fixture: column missing

Hand-labelled (PLAT.MATRIX.POLICY.DEFS REQ-003). Expected: `MissingColumn { column: Release-blocking }` only.

## Tiers

| Tier | CI obligation | Support response | Promotion/demotion |
|---|---|---|---|
| Tier 1 | Full test suite every release. | Before release. | Two green release candidates. |
| Tier 2 | Build every release. | Best effort. | Promotion rule. |
| Tier 3 | None. | Community. | By pull request. |
