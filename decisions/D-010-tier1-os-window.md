---
id: D-010
title: Tier 1 OS version window and ARM64 tiers
status: open
raised_by: PLAT.MATRIX split
blocks: [PLAT.MATRIX.POLICY]
decide_by: before the P1a support matrix is published
updated: 2026-11-16
---

# D-010: Tier 1 OS version window and ARM64 tiers

## Question

1. For the P1a Tier 1 set, does "current macOS" and "current Ubuntu LTS" mean the latest release only, or the latest two?
2. Which tier do Linux ARM64 and Windows ARM64 get, and from which phase?

## Context

REQ-PLAT-002 and REQ-PLAT.MATRIX-005 name Windows 10/11 x86-64, current Ubuntu LTS x86-64 and current macOS ARM64 as Tier 1 from P1a. Each Tier 1 row costs a CI build and a full test run on every release (REQ-PLAT.MATRIX-003), and macOS and ARM runners are the most expensive. Shop-floor PCs are often a release or two behind, so a latest-only window may exclude real users. PLAT and PLAT.MATRIX both carried the ARM64 question forward unresolved. Windows 10 reached Microsoft's end of mainstream support in October 2025 [unverified], which also interacts with the EOL demotion rule (REQ-PLAT.MATRIX-006).

## Options

| Option | For | Against |
|---|---|---|
| A. Latest only (latest Ubuntu LTS, latest macOS); ARM64 Linux and Windows Tier 3 | Cheapest CI; simplest | Excludes shops one release behind; Windows 10 question still open |
| B. Latest two (two Ubuntu LTS, two macOS majors); Linux ARM64 Tier 2, Windows ARM64 Tier 3 | Matches shop-floor upgrade lag; ARM Linux builds catch portability bugs cheaply | About twice the Tier 1 CI cost on Linux and macOS |
| C. As B, plus Windows ARM64 Tier 2 | Early signal for ARM Windows laptops | Instrument SDKs are mostly x86-64 Windows only [unverified]; little user benefit before P2 |

## Recommendation

B. Keep Windows 10 Tier 1 while Microsoft's extended security updates are available to enterprises [unverified], then demote per REQ-PLAT.MATRIX-006.

## Decision

_Open._
