## What

`spec(<NODE.ID>): split | leaf | decision` · or `seeds:` / `tools:` / `docs:`

Code PRs name the spec leaf they build on a line of its own (checked by `tools/claims.py trace`;
a maintainer may label typo, dependency-bump and CI-only PRs `no-spec` instead):

Implements: <NODE.ID>

## Checklist

- [ ] `python tools/scope.py check` passes and `generated/` is committed
- [ ] Checked, then claimed: no open PR names this node, and `claimed_by:` is set (stale claims: `python tools/claims.py stale`)
- [ ] Only the target node, its new children and `decisions/` changed (AGENTS.md rule 1)
- [ ] Requirements are testable, with one `shall` each
- [ ] No standards or vendor text pasted; unverified claims marked `[unverified]`
- [ ] Level 1–2 split: a domain maintainer is requested as reviewer
- [ ] I report only what I actually ran
- [ ] App-crate change: reviewed against the [app-crate checklist](https://github.com/metrologymaven/metrologymaven/blob/main/docs/review/app-crate-checklist.md) (`docs/review/app-crate-checklist.md`)
