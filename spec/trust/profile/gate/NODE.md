---
id: TRUST.PROFILE.GATE
title: Profile query API and enforcement hooks
level: 3
parent: TRUST.PROFILE
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE-003, REQ-TRUST.PROFILE-004, REQ-TRUST.PROFILE-005, REQ-TRUST.PROFILE-006]
covers: []
depends_on: [TRUST.PROFILE.DEFS, TRUST.PROFILE.SELECT, CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

- REQ-TRUST.PROFILE-005 needs CORE.BUS to expose principal kind and a precondition hook point; CORE.BUS lists principal kinds (REQ-CORE.BUS-001) but the hook API is not yet specified.
- "Released" export: the parent assumes a released state for packages; whether that is a W3 release of the run (CORE) or a package flag needs alignment with CORE's W3 model.

## Children

_None yet: status `open`._
