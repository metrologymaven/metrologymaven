---
id: D-014
title: Who hosts and pays for reference benchmark machines
status: accepted
raised_by: PLAT.HW split
blocks: [PLAT.HW.REFMACH, PLAT.HW.RUN]
decide_by: before the first P1a release benchmark
updated: 2026-11-17
---

# D-014: Who hosts and pays for reference benchmark machines

## Question

Who owns, hosts and pays for the physical reference machines (at least one at the minimum hardware profile and one at the recommended profile, per REQ-PLAT.HW-002) on which every release is benchmarked, and the GPU-equipped CI runners that rendering jobs need?

## Context

REQ-PLAT.HW-003 requires the benchmark suite to run on every named reference machine for every release. Hosted CI runners are virtualised and noisy, and mostly lack GPUs [unverified], so stable numbers need dedicated hardware. That costs money and someone must keep the machines patched and online. The same question arises for GPU rendering runners (`PLAT.GPU.CI`). Spending money is a maintainer decision (AGENTS.md working rule 5); D-001 says revenue comes from services and support, not closed features.

## Options

| Option | For | Against |
|---|---|---|
| A. Project-owned machines hosted by a maintainer, funded by sponsorship | Full control; stable numbers | Single point of failure; a person's home or office becomes infrastructure |
| B. Vendor-donated or vendor-hosted machines (GPU or instrument vendors) as self-hosted runners | No cost to the project; vendor goodwill | Dependence on a vendor; perceived bias in published numbers |
| C. Cloud bare-metal or GPU instances rented per release | No hardware to maintain; reproducible machine type | Recurring cost; cloud GPUs differ from shop-floor PCs |
| D. Community-run machines reporting signed results | Free; broad coverage | Hard to keep stable; trust in submitted numbers |

## Recommendation

A for the minimum-profile machine (a cheap, typical shop-floor PC) plus C for the recommended profile until sponsorship allows owning it. Publish who hosts each machine with the results.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A for the minimum-profile machine (a cheap, typical shop-floor PC) plus C for the recommended profile until sponsorship allows owning it. Publish who hosts each machine with the results.
