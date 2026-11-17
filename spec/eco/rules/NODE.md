---
id: ECO.RULES
title: House rules and their sync into every repository
level: 2
parent: ECO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-004]
covers: []
depends_on: [ECO.REPO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES: House rules and sync

## Purpose

Keeps one canonical source for contributor and agent rules and copies it into every repository as a generated block that carries the source revision and a digest. It owns the canonical rule text's structure and size budgets, the sync tool, and the tamper and staleness checks that CI calls.

## Scope

**In:**
- Canonical rules source layout (core block, language-specific blocks such as Rust, repo-local section)
- Generated block format: begin and end markers, source revision, digest
- The sync tool that writes or refreshes blocks across repositories
- The checks: edited block (fail), stale block (warn), missing block (fail)
- Size budgets for the core block and for AGENTS.md
- Agent-rule content headings adopted from the Craft family (accountability, check then claim, report only what ran, ask only for high-stakes actions)

**Out** (and which node owns it instead):
- Running the checks on PRs (owned by `ECO.CI`)
- Governance policy text and maintainer roles (GOVERNANCE.md, decided by D-003)
- RFC and PR-timeline tooling (owned by `ECO.COMM`)
- Never-crash and clean-room lint configuration (owned by `ECO.CI`; the rules text states them)

## Requirements

- **REQ-ECO.RULES-001** The project shall keep exactly one canonical source for house rules, and every other copy shall be a generated block.
- **REQ-ECO.RULES-002** Each generated block shall record the canonical source revision and a SHA-256 digest of the block body.
- **REQ-ECO.RULES-003** If a generated block's body does not match its recorded digest, then the rules check shall fail and name the file.
- **REQ-ECO.RULES-004** If a generated block's recorded revision is older than the current canonical revision, then the rules check shall emit a warning, not a failure.
- **REQ-ECO.RULES-005** If a repository lacks the required generated block, then the rules check shall fail.
- **REQ-ECO.RULES-006** The core rules block shall stay under 3 KB and each repository's AGENTS.md under 10 KB, and the rules check shall warn when either limit is exceeded.
- **REQ-ECO.RULES-007** When the sync tool runs twice with no change to the canonical source, the second run shall produce no file changes.
- **REQ-ECO.RULES-008** Every repository shall have a CLAUDE.md containing only a reference to AGENTS.md, so no tool-specific rules diverge.

## Interfaces

- **Offers:** canonical rules source format; generated block format; the sync tool; the rules check (exit codes: 0 ok, 1 fail; warnings on stdout).
- **Consumes:** the repository set from ECO.REPO.

## Decomposition hints

SRC (canonical source layout and budgets), BLOCK (block format and digest), SYNC (sync tool), CHECK (tamper, stale and missing checks).

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** ECO.RULES owns the vocabulary lint for product UI strings (OUTR.BRAND owns the outreach-material lint), and the dependency SPDX licence allowlist from D-001. Include both when this node is expanded.
- Until a `fid-contrib` repo exists, the canonical source is this repository's AGENTS.md; the block format should be in place before the second repo is created.

## Children

_None yet: status `open`._
