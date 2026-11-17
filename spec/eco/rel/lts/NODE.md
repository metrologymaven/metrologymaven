---
id: ECO.REL.LTS
title: Long-term-support channel
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-009]
covers: []
depends_on: [ECO.REL.PROC, ECO.REL.SIGN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REL.LTS: Long-term-support channel

## Purpose

Factories and regulated users can't take a new feature release every few weeks. An LTS channel gives them a fixed version with backported security and correctness fixes for a published window, while keeping kernel results bit-identical across patch releases (patch semver rule, CONSTITUTION §4).

## Scope

**In:**
- LTS cadence and support window
- Backport policy (security and correctness only)
- Kernel bit-identity gate for LTS patches
- LTS-specific release notes and validation evidence

**Out:**
- Commercial terms, pricing and contracts (business documents outside the tree)

## Requirements

- **REQ-ECO.REL.LTS-001** The project shall offer a long-term-support release channel with a published support window, security-only and correctness-only backports, and kernel bit-identity across LTS patch releases.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added in the level-3 review (2026-11-16) from the adoption gap analysis; not yet expanded.

## Children

_None yet: status `open`._
