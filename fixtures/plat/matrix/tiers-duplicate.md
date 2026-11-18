# Tiers fixture: duplicate tier row

Hand-labelled (PLAT.MATRIX.POLICY.DEFS REQ-003). Expected: `DuplicateTier { tier: tier2 }` only
(the first Tier 2 row is complete; the second, written `**tier 2**`, is the duplicate).

## Tiers

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | Full test suite every release. | Yes. | Before release. | Two green release candidates. |
| Tier 2 | Build every release. | No. | Best effort. | Promotion rule. |
| **tier 2** | Something else. | Yes. | Before release. | Promotion rule. |
| Tier 3 | None. | No. | Community. | By pull request. |
