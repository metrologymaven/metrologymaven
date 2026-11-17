---
id: TRUST.ESIG.CEREMONY
title: Signing ceremony, UI hooks and refusal paths
level: 3
parent: TRUST.ESIG
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.ESIG-001, REQ-TRUST.ESIG-002, REQ-TRUST.ESIG-005, REQ-TRUST.ESIG-006]
covers: []
depends_on: [TRUST.ESIG.MEANING, TRUST.ESIG.BIND, TRUST.ESIG.MULTI, TRUST.SIGN.UV, TRUST.PROFILE, CORE]
cli: [fid sign sign-package]
mcp: []
journal: [sign.sign_package@1, sign.refuse@1]
updated: 2026-11-16
---

# TRUST.ESIG.CEREMONY: Signing ceremony, UI hooks and refusal paths

## Purpose

The attended human act: the `sign.sign_package@1` S1 command and the step sequence any app runs to present it. Load the package, check adoption state, show exactly what will be signed, take an explicit confirmation, hand the bound statement to the user-verification gate, and append the outcome to the journal. It also defines the UI hook contract (preview data, confirm, cancel) that app GUIs implement, so every app shows the same ceremony. It is separate from BIND (what bytes are signed) and MULTI (ordering) because it is the stateful, UI-facing flow.

## Scope

**In:**
- `sign.sign_package@1` registry entry (S1, never on MCP)
- Ceremony state machine: load → adoption check → profile check → preview → confirm → UV → write envelope → journal
- Preview data model (package digest, included results, meaning, signer name, prior signatures)
- UI hook contract for app GUIs; attended interactive terminal path if CORE allows one
- Unadopted agent work check (C5) and the named refusal
- Cancel and UV-failure paths producing `sign.refuse@1`
- Writing the envelope into the package's `signatures/` folder in a new package file

**Out** (and which node owns it instead):
- Statement fields and journal event payloads (`TRUST.ESIG.BIND`)
- Sequence rules for several signers (`TRUST.ESIG.MULTI`)
- The UV prompt and key operation (`TRUST.SIGN.UV`)
- Widgets and rendering (`VIZ`)
- Which meanings are required (`TRUST.PROFILE`)

## Requirements

- **REQ-TRUST.ESIG.CEREMONY-001** The command `sign.sign_package@1` shall be class S1, shall have no MCP projection, and shall run only when the bus reports a human principal on an attended GUI session (or an attended terminal session if CORE.BUS defines one).
- **REQ-TRUST.ESIG.CEREMONY-002** Before confirmation, the ceremony shall present the statement digest, every included result (characteristic, value, decision), the selected meaning code and label, the signer's printed name and every prior signature.
- **REQ-TRUST.ESIG.CEREMONY-003** The confirm step shall be a separate user action from the OS user-verification prompt, and confirm shall be disabled until the preview has been displayed in full.
- **REQ-TRUST.ESIG.CEREMONY-004** If any included result depends on agent-authored work without a recorded human adoption, then the ceremony shall stop with `esig.unadopted` listing each item, before preview.
- **REQ-TRUST.ESIG.CEREMONY-005** If the user cancels at any step or UV fails, then the ceremony shall append one `sign.refuse@1` event with the step and reason, and shall leave every package file unchanged.
- **REQ-TRUST.ESIG.CEREMONY-006** When signing succeeds, the ceremony shall write a new package file containing the added envelope and shall never modify the input package file in place.

## Interfaces

- **Offers:** `sign.sign_package@1` (S1); `CeremonyHooks` trait (`show_preview`, `await_confirm`, `on_cancel`, `on_result`) implemented by app GUIs.
- **Consumes:** meanings (`TRUST.ESIG.MEANING`), statement and event builders (`TRUST.ESIG.BIND`), sequence validation (`TRUST.ESIG.MULTI`), the UV gate (`TRUST.SIGN.UV`), profile requirements (`TRUST.PROFILE`), adoption state and principal from `CORE`.

## Decomposition hints

FLOW (state machine + command), HOOKS (UI contract + headless test harness), ADOPT (C5 adoption check against CORE).

## Open questions

- Blocked in practice by `TRUST.SIGN.UV` (D-011) for Linux; the Windows and macOS flow can be specified first.
- The interactive-CLI signing path conflicts with REQ-CORE.BUS-002 (CLI is `script`); until D-012 (principal kind of a human at the CLI) is decided, the ceremony is GUI-only.
- Signing a report PDF versus signing the package: proposed rule is that only packages are signed and reports embed the package digest; confirm with RPT.

## Children

_None yet: status `open`._
