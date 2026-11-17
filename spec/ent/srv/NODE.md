---
id: ENT.SRV
title: Server core: results catalog, project and program store, template library
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ENT-001]
covers: []
depends_on: [CORE.BUS.PIPE, CORE.BUS.POL, CORE.JRN.CHAIN, CORE.STO.CAS, RPT.CATALOG.STORE, RPT.CATALOG.QUERY, AUTO.REC.LIFE, TRUST.PROFILE.GATE, ENT.IAM]
cli: [fid server *-results, fid server *-project, fid server *-program, fid server *-template, fid server *-site]
mcp: [server_*_results, server_*_project, server_*_program, server_*_template, server_*_site]
journal: [server.*_results@*, server.*_project@*, server.*_program@*, server.*_template@*, server.*_site@*]
updated: 2026-11-16
---

# ENT.SRV: Server core: results catalog, project and program store, template library

## Purpose

The optional, self-hosted server process and the data it holds for many users and sites: an archive of project journals and blobs, a server-side results catalog that extends the local `RPT.CATALOG` model across desktops, and shared libraries of released programs and project and report templates. It runs the same command bus and registry as the desktop, so every change to server state is a journaled registry command (C3, C4). It is a separate unit because it is the single owner of server-side state that the other ENT children read and write through commands.

## Scope

**In:**
- Server process hosting a command bus instance, its own server journal and the server command set (`server.*`)
- Project archive: per-project journals and content-addressed blobs as received from desktops, stored without rewriting
- Server results catalog: indexing of ResultRecords from archived journals, using the `RPT.CATALOG` entry and query model, partitioned by site
- Program library and template library (project templates, report templates): publish, list, fetch, lifecycle states from `AUTO.REC.LIFE`
- Sites as a server-side grouping of projects, desktops and users
- Authorisation hook: every server command checks the caller's role and site scope from `ENT.IAM`
- Server-side Access Ledger entries for reads of records by external principals (format from `CORE.JRN.STREAM`)

**Out** (and which node owns it instead):
- Sign-in, roles, directory sync and principal mapping (`ENT.IAM`)
- Moving journals and blobs between desktops and the server, offline queues and conflicts (`ENT.SYNC`)
- Outbound webhooks and the event schema (`ENT.EVENTS`)
- MES, QMS, ERP, PLM and SPC connectors (`ENT.CONNECT`)
- Cross-site dashboards (`ENT.DASH`)
- Container images, backup, restore and upgrades (`ENT.OPS`)
- Local results catalog and shared-folder catalogs (`RPT.CATALOG`)
- Signing and signature checks (`TRUST.SIGN`)

## Requirements

- **REQ-ENT.SRV-001** Every change to server-held state shall be made by a registry command executed by the server's command bus and shall append exactly one entry to the server journal.
- **REQ-ENT.SRV-002** The server shall store each received project journal byte-for-byte and shall reject any received entry whose hash-chain link does not verify against the stored chain.
- **REQ-ENT.SRV-003** Each server catalog entry shall carry its source project ID, journal entry hash and site, using the `RPT.CATALOG` entry model.
- **REQ-ENT.SRV-004** The server catalog shall answer a query over 10,000,000 indexed results filtered by part, characteristic and site in under 2 s on the reference server profile [unverified].
- **REQ-ENT.SRV-005** Publishing a program or template into a server library shall be a W2 command, and marking a library entry released shall be a W3 command that commits only for a principal allowed W3 by D-030 and the active governance profile.
- **REQ-ENT.SRV-006** If a caller's role or site scope from `ENT.IAM` does not allow a command, then the server shall refuse it with a typed `server.forbidden` error and record the denial in the Access Ledger.
- **REQ-ENT.SRV-007** The server shall never compute a value of record outside the deterministic kernel, and any statistic it serves shall name the kernel version that produced it.
- **REQ-ENT.SRV-008** A desktop shall be able to fetch a released library entry and use it with the server unreachable afterwards, with the entry's content digest checked on fetch.
- **REQ-ENT.SRV-009** Deleting and rebuilding the server catalog from the project archive shall give the same catalog content digest.

## Interfaces

- **Offers:** server bus and journal; `server.*` commands on results, projects, programs, templates and sites; library entry content digests; catalog query endpoint consumed by `ENT.DASH`, `ENT.EVENTS` and `ENT.CONNECT`; archive write API used by `ENT.SYNC`.
- **Consumes:** command bus pipeline and policy (`CORE.BUS.PIPE`, `CORE.BUS.POL`), chain verification (`CORE.JRN.CHAIN`), blob store (`CORE.STO.CAS`), catalog store and query model (`RPT.CATALOG.STORE`, `RPT.CATALOG.QUERY`), lifecycle states (`AUTO.REC.LIFE`), profile gate (`TRUST.PROFILE.GATE`), role and site checks (`ENT.IAM`).
- Network transport for clients is named at leaf level; it shall carry the shared result envelope of `AUTO.SURF.ENV`.

## Decomposition hints

HOST (server process, bus instance, server journal), ARCHIVE (journal and blob archive with chain checks), CATALOG (server catalog, site partitioning, rebuild), LIB (program and template libraries and their lifecycle), SITE (sites and scoping), AUTHZ (role and site checks at the bus).

## Open questions

- CONSTITUTION §2 lists the binaries (`fid`, apps, `fid-verify`) and has no server binary. This node assumes the server is started through the `fid` binary (`fid server ...`); a separate binary would need a constitution change.
- Who may mark a library entry released depends on D-030.
- Whether one server may host several organisations (multi-tenancy) or only one organisation with many sites is not stated in ENT; this node assumes one organisation per server.

## Children

_None yet: status `open`._
