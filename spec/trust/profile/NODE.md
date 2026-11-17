---
id: TRUST.PROFILE
title: Governance profiles
level: 2
parent: TRUST
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-005]
covers: []
depends_on: [CORE]
cli: [fid sign set-profile, fid sign show-profile]
mcp: [sign_show_profile]
journal: [sign.set_profile@1]
updated: 2026-11-16
---

# TRUST.PROFILE: Governance profiles

## Purpose

Lets a tenant choose how strictly human review and signing are enforced: **Lean** (minimal ceremony), **Regulated** (signatures with meanings, required review steps, controls that map to 21 CFR Part 11 and EU GMP Annex 11 concepts [unverified]) or **Assistive-only** (agents may only propose; no agent-run W2 commands on the working document). Profiles change GUI gating and required ceremony, never the command registry or side-effect class taxonomy. It is separate because it is configuration policy that other nodes read, not a signing mechanism.

## Scope

**In:**
- Profile definitions and their parameters (required meanings, required signer sequence, adoption rules, agent branch limits)
- Selecting and changing the active profile (A0 admin command)
- Profile digest recorded in every Result Package and journal session header
- Gating rules the GUI and command bus query
- Mapping table from Regulated controls to Part 11 / Annex 11 clause designators (citations only)

**Out** (and which node owns it instead):
- The signing ceremony itself (`TRUST.ESIG`)
- Side-effect class enforcement on the command bus (`CORE`)
- User and role management beyond the OS account (not in the open core; see D-001)
- Audit-trail storage (`CORE` journal)

## Requirements

- **REQ-TRUST.PROFILE-001** The system shall offer exactly three built-in profiles, Lean, Regulated and Assistive-only, each with a published parameter set.
- **REQ-TRUST.PROFILE-002** Changing the active profile shall be an A0 command, never projected to MCP, and shall append a `sign.set_profile@1` journal event with old and new profile digests.
- **REQ-TRUST.PROFILE-003** No profile shall add, remove or reclassify a registry command or side-effect class, and a CI test shall assert the registry manifest digest is identical under all three profiles.
- **REQ-TRUST.PROFILE-004** While the Regulated profile is active, the system shall refuse to export a Result Package as released until every signature meaning the profile requires has been applied.
- **REQ-TRUST.PROFILE-005** While the Assistive-only profile is active, the command bus shall refuse W1 and W2 commands from agent principals except creating proposals on an agent branch.
- **REQ-TRUST.PROFILE-006** Every Result Package and every journal session shall record the digest of the profile that was active when it was produced.
- **REQ-TRUST.PROFILE-007** The Regulated profile documentation shall cite each mapped control by regulation designator and section number only, and shall state that using the profile does not by itself make a system compliant.

## Interfaces

- **Offers:** `sign.set_profile@1` (A0), `sign.show_profile` (R0), a profile query API for the GUI, command bus and `TRUST.ESIG`.
- **Consumes:** principal and side-effect class data from `CORE`.

## Decomposition hints

DEFS (three profiles + schema), SELECT (A0 command + journal), GATE (query API + bus hooks), MAP (Regulated control mapping doc).

## Open questions

- D-001 (and the TRUST open question) asks whether Part 11 controls belong in the open core. This node assumes yes under D-001 option A/C; a different outcome would move the Regulated profile out.
- Tenant-defined profiles beyond the three built-ins are not specified; propose via decision if users ask.
- The Assistive-only rule in REQ-TRUST.PROFILE-005 needs CORE to expose principal type on every command; confirm against CORE's bus design.

## Children

_None yet: status `open`._
