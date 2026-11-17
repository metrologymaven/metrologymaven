---
id: ENT.CONNECT
title: Integration connectors: SDK, MES/QMS, ERP, PLM and SPC
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ENT-005]
covers: []
depends_on: [ECO.SDK.EXT, ECO.SDK.PKG, ECO.SDK.ISO, ENT.EVENTS, ENT.SRV, ENT.IAM, RPT.OUTPUT.SPC, IO.CAD.PMI]
cli: [fid integrate *-connector, fid integrate run-*]
mcp: [integrate_*_connector, integrate_run_*]
journal: [integrate.*_connector@*, integrate.run_*@*]
updated: 2026-11-16
---

# ENT.CONNECT: Integration connectors: SDK, MES/QMS, ERP, PLM and SPC

## Purpose

Connects the server, and where useful a desktop, to factory systems: MES and QMS (holds, nonconformances), ERP (part, order and lot data), PLM (CAD and PMI pull, results push) and SPC servers. Each connector is an installable plugin built on a connector profile of the SDK and passes a conformance kit before it is listed. It is a separate unit because vendor-specific integration logic must stay outside the server core and be replaceable.

## Scope

**In:**
- Connector profile: a named extension-point subset of the SDK for connectors, with inbound (pull) and outbound (push) operations
- Connector conformance kit: a mock target system per category and a test suite every connector must pass
- First-party connectors: at least one MES/QMS, one PLM and one SPC system
- ERP part, order and lot lookup used to fill run metadata
- PLM CAD and PMI pull into a project through the CAD importers; results push to PLM
- Mapping of server events to connector actions; connector run records
- Side-effect classes: every connector action that sends data off the machine is X1

**Out** (and which node owns it instead):
- Plugin packaging, signing, isolation and lifecycle (`ECO.SDK`)
- Webhook delivery and event schemas (`ENT.EVENTS`)
- AQDEF, QIF Statistics and CSV writers and desktop SPC export (`IO.RES`, `RPT.OUTPUT.SPC`)
- STEP AP242 and PMI import (`IO.CAD`)
- Service credentials (`ENT.IAM`)

## Requirements

- **REQ-ENT.CONNECT-001** The SDK shall offer a connector profile that declares each connector operation with its side-effect class, target system category and the data it reads and sends.
- **REQ-ENT.CONNECT-002** The project shall publish a conformance kit per category (MES/QMS, ERP, PLM, SPC) with a mock target, and a connector shall be listed only when it passes the kit for its category.
- **REQ-ENT.CONNECT-003** The project shall ship at least one MES/QMS, one PLM and one SPC connector, each as an installable plugin passing its kit.
- **REQ-ENT.CONNECT-004** Every connector operation that sends data to another machine shall be side-effect class X1 and request-only on MCP.
- **REQ-ENT.CONNECT-005** Data a connector pulls from a target system shall be typed as untrusted content and enter the project only through registry import commands.
- **REQ-ENT.CONNECT-006** If a target system is unreachable, then the connector shall queue or fail its operation with a typed error and shall not block any desktop or server command that does not depend on it.
- **REQ-ENT.CONNECT-007** Every connector run shall be recorded with connector ID and version, operation, target, outcome and payload digest.
- **REQ-ENT.CONNECT-008** Installing, enabling, configuring or removing a connector shall be A0 and never projected to MCP (CONSTITUTION §3.1).

## Interfaces

- **Offers:** connector profile for `ECO.SDK.EXT`; conformance kits; `integrate.*_connector` and `integrate.run_*` commands; first-party connectors.
- **Consumes:** extension points, package format and isolation (`ECO.SDK.EXT`, `ECO.SDK.PKG`, `ECO.SDK.ISO`), events (`ENT.EVENTS`), catalog queries (`ENT.SRV`), service credentials (`ENT.IAM`), SPC data-set mapping (`RPT.OUTPUT.SPC`), PMI import (`IO.CAD.PMI`).

## Decomposition hints

PROFILE (connector SDK profile), KIT (conformance kits and mock targets), MESQMS, ERP, PLM, SPC (one first-party connector each).

## Open questions

- PLM connectors depend on vendor APIs and their terms [unverified]. The first PLM target should be chosen with a design partner; if vendor terms forbid an open-source connector, that becomes a maintainer decision.
- Which MES/QMS and SPC systems come first is not chosen. An open protocol target may be the safest first choice [unverified].
- `IO.CAD.XLATE` is waiting on a decision; PLM pull of native CAD (not STEP) depends on it.

## Children

_None yet: status `open`._
