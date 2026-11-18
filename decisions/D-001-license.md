---
id: D-001
title: Project licence
status: accepted
raised_by: founder
blocks: [ECO, TRUST]
decide_by: before first public push
updated: 2026-11-16
---

# D-001: Project licence

## Question

Which licence(s) cover the code, the spec text and the reference data?

## Context

The concept workspace (`../docs/founder-decisions.md`, B3) left licensing open with options O1–O4c, leaning towards O4a (open trust core with a closed product). The founder has since chosen an **open-source, community-built** direction. That makes O2 (proprietary) and O3 (source-available) inconsistent with the goal of a self-growing ecosystem.

The Craft app family uses MIT OR Apache-2.0 with the trademark excluded, and it has drawn large contributor communities (see `research/artcraft-design-lessons.md`).

## Options

| Option | For | Against |
|---|---|---|
| A. MIT OR Apache-2.0 for everything, trademark excluded | Maximum adoption; Rust ecosystem norm; Apache's patent grant; proven by the Craft apps | Vendors can ship closed forks; nothing commercial is reserved |
| B. Apache-2.0 core + GPL/AGPL apps | Forks of the apps must stay open | Scares off instrument vendors and integrators; dual-licensing friction |
| C. Open core (A) + separate commercial services (hosted verifier, support, attestation network) | Funds the project without closing the code | Must keep a clear line between open and paid |

## Recommendation

**C, built on A.** Code is MIT OR Apache-2.0. Spec text is CC-BY-4.0. Reference datasets use their source's licence or CC-BY-4.0. The trademark is excluded from all of these. Revenue comes from services, support and certified builds, not from closed features.

Before publishing, check with counsel on patents (B4: decide on a provisional application before public disclosure).

## Decision

**Accepted (option C on A), 2026-11-16, founder.** Code: MIT OR Apache-2.0. Spec text: CC-BY-4.0. Reference datasets: source licence or CC-BY-4.0. Trademark excluded from all licences. Revenue from services, support and certified builds, not closed features.

**Conditions before first public push:** counsel review of the patent/provisional question (B4). Follow-ups: LICENSE.md updated; a dependency SPDX allowlist is to be written in ECO.RULES.
