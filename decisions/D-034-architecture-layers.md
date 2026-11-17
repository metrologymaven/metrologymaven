---
id: D-034
title: Architecture layers and artifact (non-code) dependencies
status: accepted
raised_by: level-3 review
blocks: []
decide_by: before level-3 expansion
updated: 2026-11-17
---

# D-034: Architecture layers and artifact dependencies

## Question

1. In what layer order may crates depend on each other?
2. How does the tree record that a node *reads another node's output files* (a test suite, a manifest, a report) without that counting as a code dependency?

## Context

Agents in at least 9 domains (ALN, GDT, UNC, TRUST, ECO, PLAT, VIZ, KRN, IO) reported the same problem. Real dependencies would close loops, so they replaced them with prose "interface notes". Examples:

| Loop or conflict | Why it happens |
|---|---|
| GDT.RES ⇄ UNC.ORS | ORS *test data* cites DMT rows, but GDT consumes UNC decisions |
| ECO.DATA ⇄ PLAT.HW | ECO's benchmark runner runs PLAT's suite, while PLAT's CI runs on ECO |
| VIZ ⇄ RPT, PLAT, OUTR | They consume VIZ, and VIZ wants their profiles and templates |
| KRN → GEO → CORE | The kernel must not depend on CORE, but GEO.TYPES does |
| TRUST.PKG → UNC → … → TRUST.ESIG | Records flow both ways |

Prose notes are invisible to `scope.py`, so these hand-offs can silently break. Every case is one of two things: a missing layer rule, or an **artifact** dependency (reading a file another node publishes) that is mistaken for a code dependency.

## Proposal

**A. Layers.** Each node's crates sit in one layer, and may depend only on the same or lower layers.

| Layer | Contents | May not depend on |
|---|---|---|
| L0 Foundation | Pure libraries: KRN.*, GEO geometry libraries (everything except `*.CMD` and storage), UNC math (MODEL, PROP, XFM), unit catalogue, canonical JSON (CORE.JRN.CANON), HAL.IDL types | CORE or anything above |
| L1 Core | CORE.* (registry, bus, journal, storage, jobs, project) | L2, L3 |
| L2 Domain services | Command crates of IO, GEO.*.CMD, ALN, GDT, UNC commands, SCAN, RPT, HAL runtime, CMM, LVM, TRUST, IND, MIG, ENT services, in this **order**: IO < ALN < GDT < UNC-commands < SCAN < RPT < TRUST < HAL < CMM < LVM < IND < MIG < ENT | Any later L2 domain in the order, or L3 |
| L3 Surfaces and apps | AUTO, VIZ, ECO.APPS, FLOW runners, AGENT, I18N string tooling, UX tokens consumer code | — |
| Tooling (out of band) | ECO.CI/REL/RULES/PARITY, OUTR, PLAT checks, QUAL evidence, UNC.RDL/ORS suites, test harnesses | Use **artifacts** only, never crate dependencies on product code |

**B. Artifact dependencies.** Add a frontmatter field `uses_artifacts: [NODE.ID, ...]` that means "reads a file or format this node publishes". It is validated (the target must exist) but is **not** part of cycle detection. The publisher's Interfaces section must name the file and its schema version. Test suites, reference data, manifests, parity and benchmark reports, and design tokens all travel this way.

`scope.py` already supports `uses_artifacts:` (warnings for unknown targets, excluded from cycles). The layer table would be enforced at implementation time by ECO.REPO.LAYERS.

## Options

| Option | For | Against |
|---|---|---|
| A + B as proposed | Removes almost every reported loop; makes hand-offs checkable | Agents must re-tag about 40 prose notes during the next expansion |
| B only | Smaller change | Leaves the KRN/GEO/CORE layering ambiguous |
| Neither | No work | Interface notes keep multiplying and drift |

## Recommendation

A + B. In the next expansion, every agent converts its "interface note instead of dependency" text into `uses_artifacts:` or a layer-legal `depends_on:`.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A + B. In the next expansion, every agent converts its "interface note instead of dependency" text into `uses_artifacts:` or a layer-legal `depends_on:`.
