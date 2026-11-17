---
id: ECO.APPS.POLICY
title: App-policy boundary and cross-app command identity
level: 3
parent: ECO.APPS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-003]
covers: []
depends_on: [ECO.APPS.MANIFEST, ECO.REPO, CORE.REG]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.APPS.POLICY: App-policy boundary and cross-app command identity

## Purpose

Enforces the two rules that keep a family of apps from turning into several diverging products: app crates contain no measurement, fitting, evaluation or file-format logic and define no registry commands of their own; and shared crates contain no app policy (no branching on which app is running). It also proves that a command reachable in two apps is the same command in both. The checks are repository tools run by ECO.CI.

## Scope

**In:**
- The rule that every registry command is defined in a shared crate, never in an app crate
- The rule that shared crates never reference app identifiers or app-specific configuration
- The cross-app identity check: same command ID implies same parameter schema, result schema and side-effect class in every app that enables it
- Reviewer checklist text for the parts a tool cannot decide

**Out** (and which node owns it instead):
- Layer assignments and the general layering check (owned by `ECO.REPO`)
- Manifest schema (owned by `ECO.APPS.MANIFEST`)
- Running the checks on PRs (owned by `ECO.CI`)

## Requirements

- **REQ-ECO.APPS.POLICY-001** If the registry manifest attributes any command to a crate in the app layer, then the policy check shall fail and name the command and crate.
- **REQ-ECO.APPS.POLICY-002** If a shared-layer crate references an app identifier from the app list or reads app manifest content other than through the generic loader interface, then the policy check shall fail and name the location.
- **REQ-ECO.APPS.POLICY-003** When the per-app effective registries are computed from the manifests, the policy check shall fail if any command ID appears in two apps with differing parameter schema digest, result schema digest or side-effect class.
- **REQ-ECO.APPS.POLICY-004** The project shall publish a reviewer checklist for app-crate PRs that covers the cases the automated checks cannot decide (for example numeric logic hidden in UI code).

## Interfaces

- **Offers:** the policy check (repository tool), its findings format, the reviewer checklist.
- **Consumes:** CORE.REG registry manifest (command ID, defining crate, schema digests, side-effect class); ECO.REPO layer map; ECO.APPS.MANIFEST manifests.

## Decomposition hints

DEFN (commands defined only in shared crates), LEAK (no app identifiers in shared crates), IDENT (cross-app identity check), CHKLIST (reviewer checklist).

## Open questions

- REQ-001 and REQ-003 need the registry manifest to record the defining crate and schema digests per command. CORE.REG is open; this requirement should be raised with CORE.REG when it is expanded (cross-domain).

## Children

_None yet: status `open`._
