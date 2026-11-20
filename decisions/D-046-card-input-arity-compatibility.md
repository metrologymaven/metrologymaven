---
id: D-046
title: Spec-card input_arity and algorithm version compatibility
status: accepted
raised_by: fix of KRN.MAN.KCI (input-arity check, PR 315)
blocks: []
decide_by: before PR 315 merges
updated: 2026-11-20
---

# D-046: Spec-card input_arity and algorithm version compatibility

## Question

When a spec card gains or changes `input_arity` (REQ-KRN.MAN.CARD-006), must the algorithm's
semver change? More generally, which card changes need an algorithm version bump?

## Context

- `KRN.MAN.KCI` `evaluate` used to pass any number of inputs to `run`. PR 315 adds a required
  `input_arity` field to every card (REQ-KRN.MAN.CARD-006 .. -008), and `evaluate` now refuses
  calls outside it (REQ-KRN.MAN.KCI-008). The field is in the canonical card bytes, so every
  card digest changed.
- An algorithm ID `<module>.<feature>.<method>@<semver>` identifies what a ResultRecord was
  computed with. If the set of calls an ID accepts could change without a version change, two
  replays of one call under the same ID could give a result in one kernel and a refusal in
  another.
- KRN.MAN.CARD stated no versioning rule before this record.
- At the time of this change no release manifest (`KRN.MAN.MANI`) has been published, so no
  published card digest exists. The existing algorithms (`fit.{point,line2,line3,plane}.ls@1.0.0`
  and the test stand-ins) give bit-identical outputs for every call they still accept.

## Options

| Option | For | Against |
|---|---|---|
| A: bump every algorithm whose card changed | Simple rule | Churns IDs for a pure declaration change before any release; every fixture keyed by ID changes |
| B: no bump for this introduction, a bump for any later change to the accepted calls of a released algorithm | IDs stay stable while nothing is published; after release, an ID pins the accepted call set | Needs the release condition to be stated and checked by review |

## Decision

**Option B**, accepted 2026-11-20 by the orchestrator under the maintainer's delegation.

1. **Introduction of `input_arity` (PR 315).** No algorithm ID is bumped. This holds because
   outputs for every accepted call stay bit-identical and no release manifest or card digest
   has been published.
2. **From now on**, any change to a **released** algorithm's `input_arity`, or any other card or
   code change that narrows or widens the set of calls the algorithm accepts (inputs, parameters,
   seed use), requires an algorithm semver bump: a new ID with its own card. The old ID's card
   and behaviour stay unchanged. A released algorithm is one listed in a published kernel
   manifest.
3. A card change that leaves the accepted calls and their outputs unchanged (wording, references,
   validation evidence) needs no bump. It changes the card digest, which the manifest records.
4. Every schema or canonical-bytes change to cards is listed with its date in the "Schema changes"
   section of `spec/krn/man/card/NODE.md`.

## Consequences

- `spec/krn/man/card/NODE.md` "Schema changes" links this record.
- #311 (`unc.prop.mcm_adapt@1.0.0`) merged first; PR 315 adds its `input_arity` (`"2"`).
  #317 (`fid-xart` test algorithms) also merged first; PR 315 gives them `"0.."` (echo) and `"0"`
  (recurse). These algorithms are unreleased, so rule 1 applies to them too.
- Amending this rule needs a new decision record whose frontmatter lists `D-046` in `amends:`.
