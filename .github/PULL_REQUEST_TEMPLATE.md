## What

`spec(<NODE.ID>): split | leaf | decision` · or `seeds:` / `tools:` / `docs:`

Code PRs name the spec leaf they build on a line of its own (checked by `tools/claims.py trace`;
a maintainer may label typo, dependency-bump and CI-only PRs `no-spec` instead):

Implements: <NODE.ID>

## Checklist

- [ ] `python tools/scope.py check` passes (do not commit regenerated `generated/` files; see `D-042`)
- [ ] Checked, then claimed: no open PR names this node, and `claimed_by:` is set (stale claims: `python tools/claims.py stale`)
- [ ] Only the target node, its new children and `decisions/` changed (AGENTS.md rule 1)
- [ ] Requirements are testable, with one `shall` each
- [ ] No standards or vendor text pasted; unverified claims marked `[unverified]`
- [ ] Spec split, leaf, decision, or implementation PR: reviewed by an independent agent reviewer (`D-052`, `AGENTS.md` rule 8) and required CI checks pass
- [ ] I report only what I actually ran
- [ ] App-crate change: reviewed against the [app-crate checklist](https://github.com/metrologymaven/metrologymaven/blob/main/docs/review/app-crate-checklist.md) (`docs/review/app-crate-checklist.md`)

## Design review (only if this PR changes a path matching `design/review/paths.toml`; non-design PRs do not require human design review per `D-052`)

- [ ] Every changed design artefact has an `originality@1` declaration (`design/review/checklist.md`)
- [ ] For PRs matching `design/review/paths.toml`: `fid-design review-check` (`UX.SYSTEM.REVIEW` / `design/review/checklist.md`, unamended by `D-052`) requires a `design-review@1` record in `design/review/records/` with `"principal": "human"`
- [ ] `fid-design review-check --changed <changed paths, relative to design/> --root design` exits 0
