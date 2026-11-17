---
id: TRUST.PROFILE.GATE
title: Profile query API and enforcement hooks
level: 3
parent: TRUST.PROFILE
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE-003, REQ-TRUST.PROFILE-004, REQ-TRUST.PROFILE-005, REQ-TRUST.PROFILE-006]
covers: []
depends_on: [TRUST.PROFILE.DEFS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PROFILE.GATE: Profile query API and enforcement hooks

## Purpose

Where the active profile takes effect: a read-only query API that the GUI, command bus, exporter and signing ceremony call, plus the specific hooks the profiles need: refusing a released export under Regulated until required meanings are applied, refusing agent W1/W2 under Assistive-only except proposals on an agent branch, and stamping the profile digest into journal session headers and packages. It also owns the CI test that the registry manifest is identical under all three profiles.

## Scope

**In:**
- `ProfileQuery` API (active document, digest, typed accessors)
- Bus precondition hook for Assistive-only
- Release-export precondition for Regulated (checked against the package's signature sequence)
- Session-header and package stamping of the profile digest
- Registry-invariance CI test
- GUI gating flags (which controls to hide or require)

**Out** (and which node owns it instead):
- Side-effect class enforcement in general (`CORE.BUS`)
- The ceremony (`TRUST.ESIG.CEREMONY`)
- Writing the package (`TRUST.PKG`)

## Requirements

- **REQ-TRUST.PROFILE.GATE-001** A CI test shall build the registry manifest under each of the three profiles and fail if the three manifest digests differ.
- **REQ-TRUST.PROFILE.GATE-002** While Regulated is active, a request to mark a Result Package released shall be refused with `profile.signatures_missing` naming each required meaning not yet applied in order.
- **REQ-TRUST.PROFILE.GATE-003** While Assistive-only is active, the bus hook shall refuse W1 and W2 commands from agent principals with `profile.agent_write_forbidden`, except commands that create or update a proposal on that agent's branch.
- **REQ-TRUST.PROFILE.GATE-004** Every journal session header and every exported package shall carry the active profile digest, and a session whose profile changes mid-way shall record the change as the `sign.set_profile@1` event.
- **REQ-TRUST.PROFILE.GATE-005** The query API shall be read-only and shall return the same answers for the same profile digest.

## Interfaces

- **Offers:** `ProfileQuery`; bus precondition hook; export precondition; GUI gating flags.
- **Consumes:** profile documents (`TRUST.PROFILE.DEFS`), active selection (`TRUST.PROFILE.SELECT`), principal kind and command class from `CORE.BUS`, session headers from `CORE.JRN`.

## Decomposition hints

QUERY (API + GUI flags), BUSHOOK (Assistive-only hook, needs CORE.BUS hook point), RELEASE (Regulated export precondition), STAMP (digest into headers and packages), INVAR (registry invariance test).

## Open questions

- REQ-TRUST.PROFILE-005 needs a CORE.BUS hook point. CORE.BUS.POL.CLASS defines a fixed class/principal matrix but no extension hook for a tenant policy; `TRUST.PROFILE.GATE.ASSIST` specifies the hook it needs (`PolicyHook`, called at the authorise stage) and CORE.BUS must add it [cross-domain, unresolved].
- REQ-TRUST.PROFILE.GATE-004 needs a `profile_digest` field in the `session_header` entry. CORE.JRN.ENTRY (REQ-CORE.JRN.ENTRY-006) records only registry and kernel manifest digests and the app version today; `TRUST.PROFILE.GATE.STAMP` supplies the value and CORE.JRN.ENTRY must add the field [cross-domain, unresolved].
- "Released" package: there is still no single W3 release command; `TRUST.PROFILE.GATE.RELEASE` offers a pure precondition that whichever W3 release command (the ESIG ceremony or a CORE run release) calls before releasing.
- Dependencies narrowed (2026-11-17, D-034): this module depends only on `TRUST.PROFILE.DEFS`; each child names its own CORE leaves, and the registry-invariance test (`INVAR`) reads artifacts only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.PROFILE.GATE.QUERY | Read-only profile query API and GUI gating flags | 005 | none |
| TRUST.PROFILE.GATE.ASSIST | Assistive-only agent-write bus hook | 003 | none |
| TRUST.PROFILE.GATE.RELEASE | Regulated release precondition | 002 | none |
| TRUST.PROFILE.GATE.STAMP | Profile digest stamping of sessions and packages | 004 | none |
| TRUST.PROFILE.GATE.INVAR | Registry-invariance CI test across profiles | 001 | none |
