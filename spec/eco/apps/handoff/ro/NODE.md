---
id: ECO.APPS.HANDOFF.RO
title: Edit guard and read-only provenance for foreign elements
level: 4
parent: ECO.APPS.HANDOFF
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.HANDOFF-002, REQ-ECO.APPS.HANDOFF-003]
covers: []
depends_on: [ECO.APPS.HANDOFF.EDITORS, ECO.APPS.MANIFEST.SCHEMA, CORE.BUS.PIPE, CORE.PRJ.TREE, CORE.JRN.ENTRY, VIZ.SHELL.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.HANDOFF.RO: Edit guard and read-only provenance for foreign elements

## Purpose

When an app opens a project that holds elements made by commands it does not enable (for example a surface comparison opened in the Report app), those elements must stay visible but untouchable. This leaf provides two pieces that every app installs at start-up: an **edit guard** on the command bus that refuses any command targeting a foreign element before anything is journaled, and a **provenance view model** that tells the shell's element tree and inspector to show such elements read-only with their creating command, author principal and journal entry. It adds no registry command.

## Scope

**In:**
- Classifying each element as `own` or `foreign` from its creating command's namespace and the app's effective namespaces
- The edit guard: a pre-dispatch check installed on the bus by the app, refusing with `APP-FOREIGN-ELEMENT`
- The refusal payload: element ID, creating namespace, and the editor list from `app.list_editors@1`
- The provenance view model consumed by VIZ.SHELL.TREE (read-only flag, creating command ID, principal, journal entry reference)

**Out:**
- Listing editors (owned by `ECO.APPS.HANDOFF.EDITORS`)
- Drawing panels and badges (owned by `VIZ.SHELL.TREE`)
- The bus pipeline and its hook mechanism (owned by `CORE.BUS.PIPE`)
- Commands that only read (R0, R1) are never blocked; that is a rule here, but class data comes from `CORE.REG`

## Requirements

- **REQ-ECO.APPS.HANDOFF.RO-001** When an app opens a project, the classifier shall mark an element `foreign` if and only if the namespace of its creating command is not in the app's effective namespaces.
- **REQ-ECO.APPS.HANDOFF.RO-002** If a command of class W1, W2 or W3 names a `foreign` element among its targets, then the edit guard shall refuse it with `APP-FOREIGN-ELEMENT` before execution, and the Record Journal head shall be unchanged.
- **REQ-ECO.APPS.HANDOFF.RO-003** When the edit guard refuses a command, the refusal shall carry the element ID, its creating namespace and the result of `app.list_editors@1` for that namespace.
- **REQ-ECO.APPS.HANDOFF.RO-004** The edit guard shall let R0 and R1 commands on `foreign` elements pass unchanged.
- **REQ-ECO.APPS.HANDOFF.RO-005** For every `foreign` element, the provenance view model shall give `read_only = true`, the creating command ID, the author principal and the journal entry hash of the creating entry.
- **REQ-ECO.APPS.HANDOFF.RO-006** If an element's creating entry cannot be found in the journal, then the classifier shall mark it `foreign` with provenance `unknown`, so an element of unknown origin is never editable.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/handoff/mixed.fidproj` (element `E-cmp` created by `compare.run_surface@1`, element `E-rep` created by `report.create_report@1`) and a test app whose manifest enables `project` and `report`, when the project opens, then `E-cmp` is `foreign` and `E-rep` is `own`.
- **AC-2** (error path) Given AC-1, when the test app dispatches a W2 command that renames `E-cmp`, then it fails with `APP-FOREIGN-ELEMENT`, the refusal lists the editors from `fixtures/handoff/installs/` (`inspect` 0.2.0), and the journal head digest equals the digest before the attempt.
- **AC-3** Given AC-1, when an R0 command that describes `E-cmp` runs, then it succeeds.
- **AC-4** Given AC-1, the provenance view model for `E-cmp` gives `read_only = true`, command `compare.run_surface@1`, the recorded principal and the creating entry hash from the fixture journal.
- **AC-5** (unknown origin) Given `fixtures/handoff/orphan.fidproj` where an element's creating entry is missing, then that element is `foreign` with provenance `unknown` and a W2 command on it is refused.
- **AC-6** (determinism) Classifying the fixture project twice gives identical results in element-ID order.

## Design

- **Registry command(s):** none. `side_effect: R0` records that the leaf itself only reads; refusals happen before any write.
- **Data structures:** `ElementOrigin { element_id, creating_command, namespace, principal, entry_hash } | Unknown`; `ForeignSet` (BTreeSet of element IDs); `ProvenanceView { element_id, read_only, origin }`; refusal `AppForeignElement { element_id, namespace, editors }`.
- **Algorithm:** at project open, walk the element tree and look up each element's creating entry; compare its namespace with the app's effective namespaces from the loaded manifest; on each dispatch, the guard reads the command's declared targets (element IDs in parameters marked as targets by the registry schema) and its class; refuse if any target is foreign and the class is W1, W2 or W3. Recompute the foreign set incrementally when commands create elements.
- **Errors:** `APP-FOREIGN-ELEMENT`, `APP-ORIGIN-LOOKUP` (journal read failure; the element is treated as foreign).
- **Determinism:** D0.
- **Performance budget:** classification of 100 000 elements under 200 ms; guard check under 10 µs per dispatch [unverified until measured].
- **Crate:** `fid-app-family` (module `readonly`).

## Test fixtures

`fixtures/handoff/mixed.fidproj` and `orphan.fidproj`, built by a fixture generator in the test suite from a seeded script (MIT OR Apache-2.0); reuses `fixtures/handoff/installs/` from ECO.APPS.HANDOFF.EDITORS.

## Build prompt

> In crate `fid-app-family` (implementation repository), add module `readonly`. (1) A classifier that, given the open project's element tree, the journal, and the app's effective namespaces (from the `fid-app-manifest` loader), marks each element `own` or `foreign`: foreign when the namespace (prefix before the first dot) of the element's creating command is not enabled, or when the creating entry cannot be found (provenance `unknown`). (2) An edit guard installed through the command bus's pre-dispatch hook: for commands of class W1, W2 or W3 whose declared target element IDs include a foreign element, refuse with `APP-FOREIGN-ELEMENT {element_id, namespace, editors}` (editors from `app.list_editors@1`) before execution, so the Record Journal head does not change; R0 and R1 pass. (3) A provenance view model `{element_id, read_only, origin: {creating_command, principal, entry_hash} | unknown}` for the shell's element tree and inspector. No registry commands. Tests for AC-1..AC-6 of spec node ECO.APPS.HANDOFF.RO with fixture projects generated from a seeded script. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), BTreeMap/BTreeSet only, fail closed (unknown origin is never editable).

## Open questions

- Cross-domain (CORE.BUS.PIPE, open): needs a pre-dispatch guard hook an app can install, and a way to read a command's target element IDs from its parameter schema. Raise when CORE.BUS.PIPE is expanded.
- Cross-domain (CORE.PRJ.TREE, open): assumes each element records the journal entry that created it.

## Children

_Leaf: none._
