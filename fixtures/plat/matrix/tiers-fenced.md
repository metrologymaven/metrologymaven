# Tiers fixture: fenced code blocks are ignored

Hand-labelled (PLAT.MATRIX.POLICY.DEFS REQ-003). Expected: `MissingTier { tier: tier3 }` only.
The fenced block holds a heading and a complete table; neither counts. The first real table
after the fence lacks Tier 3.

```markdown
## Tiers
```

## Tiers

~~~markdown
## Other

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | a | b | c | d |
| Tier 2 | a | b | c | d |
| Tier 3 | a | b | c | d |
~~~

| Tier | CI obligation | Release-blocking | Support response | Promotion/demotion |
|---|---|---|---|---|
| Tier 1 | Full test suite every release. | Yes. | Before release. | Two green release candidates. |
| Tier 2 | Build every release. | No. | Best effort. | Promotion rule. |
