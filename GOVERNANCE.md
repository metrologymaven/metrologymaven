# Governance

Status: **accepted** in [D-003](decisions/D-003-governance-model.md) on 2026-11-16, together with the RFC timings below.

## Steward

Until a foundation is chosen under [D-003](decisions/D-003-governance-model.md), the project is stewarded by **Metrology Maven** (the founder's company), which holds the trademark ([D-001](decisions/D-001-license.md), [D-039](decisions/D-039-metrology-maven-name-and-public-home.md)) and receives grants and contracts on the project's behalf ([D-036](decisions/D-036-funding-alignment.md) rule 1).

## RFC timings

- A maintainer opens the comment period on a decision record or constitution change.
- The period is **3 days**, or **7 days** for adding or removing a domain or app, or changing this process.
- A maintainer other than the author must approve.
- Every objection must say what would resolve it.
- The founder (later the steering group) may decide an objection or close a record early, giving the reason in writing.

## Roles

| Role | Who | Can |
|---|---|---|
| **Contributor** | Anyone with a merged PR | Claim and expand nodes, review |
| **Domain maintainer** | Listed in a level-1 node's `owners:` | Approve level 1–2 splits in their domain, freeze nodes, triage decisions |
| **Steering group** | The founder, until there are 5 domain maintainers; then 3–5 people elected by maintainers | Amend the constitution, accept decision records, add or remove domains |

## Becoming a maintainer

A contributor can be nominated for a domain once they have, in that domain:
- merged 5 node expansions, or 3 expansions plus 5 substantive reviews, and
- had no reverted splits in the last 60 days.

Any existing maintainer or the steering group can nominate. The nomination passes with lazy consensus after 7 days.

## Decisions

- Every non-local choice (licensing, naming, phase moves, dropping a competitor feature, constitution changes) is a record in [`decisions/`](decisions/), made from [templates/DECISION.md](templates/DECISION.md).
- A record is `open` → `proposed` (with a recommendation) → `accepted` / `rejected` / `superseded`.
- The steering group accepts or rejects records. Anything urgent is decided within 14 days or escalates to the founder.

## Keeping it self-sustaining

- The tree is the backlog. No private roadmap exists outside `spec/` and `decisions/`.
- CI (`tools/scope.py check`) enforces traceability, so reviewers can focus on judgement rather than bookkeeping.
- Every level-1 domain should get at least two maintainers before implementation starts in that domain.
