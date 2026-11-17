---
id: D-019
title: Native CAD formats: plugin-only translators or out of scope
status: open
raised_by: IO.CAD split
blocks: [IO.CAD.XLATE]
decide_by: before IO.CAD.XLATE is expanded (P1b)
updated: 2026-11-16
---

# D-019: Native CAD formats: plugin-only translators or out of scope

## Question

Does the platform support native CAD formats (CATIA, NX, Creo, SolidWorks, Parasolid, JT) at all, and if so, only through third-party translator plugins registered with the format-plugin interface, never with a native-format reader in the core?

## Context

Three incumbents advertise native CAD import (seeds GOM.IMP.06, PW.IMP.03, PCD.CAD.01, SA.CAD.01). Those readers rely on licensed commercial translator libraries [unverified: per-vendor licensing terms]. Writing first-party readers for undocumented native formats would break the clean-room rule (CONSTITUTION §8) and the licence outcome of D-001 (MIT OR Apache-2.0 code) cannot carry proprietary translator SDKs. JT has a published ISO specification (ISO 14306) [unverified: edition and public availability], but the concept docs defer JT to P4 through the SDK.

REQ-IO.CAD-011 already assumes plugin-only. IO.CAD's open question asks that the assumption be confirmed. Blocks `IO.CAD.XLATE`; also affects `IO.REG.PLUGIN` (it may need a native out-of-process host, not only a WASM sandbox, because translator SDKs are native libraries) and ECO (marketplace listing of paid translators).

## Options

| Option | For | Against |
|---|---|---|
| A. Plugin-only: the core defines a translator contract (native file → STEP AP242 or → GEO/QIF entities); vendors or users supply licensed translators as plugins, installed by A0 commands | Matches incumbents' parity rows; keeps the core clean-room and licence-clean; users with existing translator licences can use them | Needs a native-plugin host and isolation; support burden for third-party plugins; parity reports must say "via plugin" |
| B. Out of scope: users convert to STEP AP242 in their CAD system; seeds marked out of scope with a reason | Simplest; no native host; STEP AP242 with semantic PMI is the open exchange path | Visible parity gap against all three incumbents; some users cannot export clean AP242 |
| C. A, plus a first-party JT reader later (P4) from the published ISO spec | Covers the one native-adjacent format with an open spec | Large effort; JT spec licensing and completeness [unverified] |

## Recommendation

A now, with C revisited in P4. The contract is small and keeps the parity row honest ("via plugin"), and B remains the documented fallback for users without a translator.

## Decision

_Open._ When decided: the option, who decided, the date, and follow-ups: set `IO.CAD.XLATE` to `open` (A or C) or move GOM.IMP.06 and the native parts of PW.IMP.03, PCD.CAD.01 and SA.CAD.01 to `seeds/out-of-scope.md` (B); update `IO.REG.PLUGIN` host kinds.
