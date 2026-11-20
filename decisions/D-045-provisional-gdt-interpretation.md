---
id: D-045
title: Provisional GD&T interpretation, rule sign-off and review pacing
status: accepted
raised_by: maintainer
blocks: [GDT.RES.STATUS, GDT.RES.PACE, GDT.CHAR.INTERP, OUTR.GDTC]
decide_by: before any GD&T result is released from a table that holds rows not signed off
updated: 2026-11-19
---

# D-045: Provisional GD&T interpretation, rule sign-off and review pacing

**Status: ACCEPTED 2026-11-19, maintainer.** Recorded by an agent from the maintainer's
written decision; it takes effect when the pull request that adds it merges.

## Question

GD&T evaluation code and the Dialect Mapping Table (DMT) rows it runs on can be written by
agents much faster than credentialed experts can review the rows (D-009, gate H4). How does
a result say which interpretation rules it rests on and whether an expert has signed them
off? And how is engine work kept from running ahead of expert review?

## Context

- An **interpretation rule** is a DMT row (`GDT.RES`, D-009) or a defaults profile, the
  named set of variant choices a characteristic evaluates under (`GDT.CHAR.DPROF`; the
  community calls it an *interpretation profile*). Both are interpretation, not code.
- `GDT.RES.ROWFLOW` already gives a row two-reviewer sign-off (D-009), edition-held
  attestations (D-016) and a reset on any content change. A released DMT table holds only
  signed-off rows (`GDT.RES.ROWASME.GATE`). Nothing yet says what a result computed from a
  row that is not signed off looks like. Development tables and user-local profiles exist
  today, so such results can be produced.
- A ResultRecord carries the primary DMT row ID, chain digest and DMT digest
  (REQ-GDT.RES-002). It does not list the other rules an evaluation used (for example the
  datum-frame and size rows behind a position at MMC) or their review status.
- Two existing maintainer principles apply: **no silent wrong results**, and **claimed values
  never look measured** (`UNC.UEC.CLASS`, `RPT.MODEL.FMT`). An unreviewed interpretation that
  looks settled breaks both.
- Gate H4 (CONSTITUTION §3.2) makes interpretation sign-off a human act. AUTO.REC.ADOPT is
  the pattern for a human-only act on an agent-touched object: `human_only: true`, the bus
  refuses agent, script, service and device principals, MCP projects the command as
  request-only, the act names the digest the human reviewed, and a later change drops it.

## Options

| Option | For | Against |
|---|---|---|
| A. Refuse every evaluation that uses a rule not signed off | Simplest; nothing unreviewed is ever shown | Engines cannot be exercised end to end before review; review would have nothing concrete to look at |
| B. Allow it, labelled `interpretation: provisional` everywhere the result goes, with sign-off bound to the rule's digest, and pace engine work to review | Engines and cases move; nothing unreviewed looks settled; review stays the throughput limit by design | Every surface (API, reports, exports, release) must carry the label |
| C. Allow it unlabelled in non-released results only | Least work | A provisional value copied out of a scratch result looks settled; breaks "no silent wrong results" |

## Decision

**B.**

1. **Rules used.** Every GD&T evaluation result carries the ID and version of each
   interpretation rule it used: every DMT row (with its revision and content digest) and
   the profile, not only the primary row.
2. **Provisional.** A rule without expert sign-off for its current content digest is
   *provisional*. A result that used any provisional rule is labelled
   `interpretation: provisional` in the API result, in every report and in every export.
   It may enter a released record only with that label visible. It is never presented as
   settled: no surface may drop, hide or abbreviate the label away.
3. **Sign-off.** Expert sign-off is the two-reviewer `signed_off` state of
   `GDT.RES.ROWFLOW` (D-009, D-016), and gate H4. It is human-only under the same
   person/agent branch rule as AUTO.REC.ADOPT. Each sign-off is journaled, names the
   rule's content digest the reviewer read (a mismatch fails with `stale_review`), and is
   invalidated by any change to the rule's content. Profiles are signed off through the
   same workflow (the row-like key proposed in `GDT.RES.ROWASME.DPROF`).
4. **Statuses.** A rule's public status is one of `draft` (an agent-authored revision no human
   has re-proposed; it is never evaluated), `provisional` or `signed_off`.
5. **Pacing.** No GD&T work beyond the **first-review subset** until every rule of that
   subset is signed off. The subset:
   - datum reference frames from planar and cylindrical datum features at RMB (no datum
     targets, no common datums, no MMB or LMB);
   - position, both RFS and at MMC with bonus tolerance (with the size rows those need);
   - flatness;
   - profile of a surface, single segment (no composite profile).

   "GD&T work" means new or changed DMT rows outside the subset, and claiming or marking
   built any `GDT` leaf that serves only callouts outside the subset. Shared machinery
   (schema, resolver, workflow, record, zone engine, lint) and leaves built before this
   decision are not paced. `GDT.RES.PACE` enforces this in CI from explicit key and leaf
   lists, deciding completeness only from a status register it has verified.
6. **Trust.** A status register counts only when it is rebuilt from the journal and matches
   the register and roster digests the project publishes; otherwise every rule is
   `provisional` (`unverified_register`). Rule digests are always recomputed from the row
   actually evaluated.
7. **Rationale.** Agent build speed must not outrun expert review.

## Consequences

- New leaves: `GDT.RES.STATUS` (status register and sign-off binding), `GDT.CHAR.PDIFF`
  (one data set under two profiles, per-rule difference), `GDT.RES.PACE` (pacing gate) and
  `GDT.CHAR.INTERP` (the `interpretation` block of the ResultRecord), and `IO.RES.ILABEL`
  (exports refuse a provisional record a format cannot label). New requirements REQ-GDT-009
  and REQ-GDT-010 trace the GDT leaves.
- `GDT.RES.ROWFLOW.REVIEW` gains the reviewed-digest check (`stale_review`) and refuses
  quoted standards text (`standards_quote`).
- `RPT.MODEL.FMT` gains the `ProvisionalInterpretation` label, which its existing
  label rule (REQ-RPT.MODEL.FMT-014) makes every renderer show.
- REQ-IO.RES-013 and -014: every results export carries the `interpretation` label, and a
  provisional record is refused (`interpretation_label_unsupported`) by a format that cannot
  carry it. REQ-RPT.FORMS.FINAL-006 and REQ-AGENT.GATEPREP-002: a release, including one
  from the H2 batch, is refused while a provisional row is rendered without its label.
- The subset includes profile of a surface (`GDT.PROF.SURF`, phase P1b) and cylindrical
  datum features, which the P1a ASME inventory (`GDT.RES.ROWASME`) does not list. This
  decision does not move phase tags. The inventory and phase tags are to be aligned in a
  separate change; until then the subset's rows outside P1a are authored ahead of their
  phase and the P1a rows outside the subset wait.
- The community side (public rule pages, profiles, cases, reviewers, benchmark suites and
  guardrails) is specified in `OUTR.GDTC`.
- `REQ-OUTR.GDTC-008` narrows D-021 option B for GD&T: the project does not publish, host or
  aggregate third-party comparisons with named products either.
- Amending the subset or the pacing rule needs a new decision record whose frontmatter lists
  `D-045` in `amends:`.
