---
id: ENT
title: Enterprise: multi-user server, identity and system integration
level: 1
parent: FID
kind: domain
status: split
phase: P1b
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-028]
covers: []
depends_on: [CORE, RPT, TRUST, AUTO]
cli: [fid server *, fid integrate *]
mcp: [server_*, integrate_*]
journal: [server.*, integrate.*]
updated: 2026-11-16
---

# ENT: Enterprise

## Purpose

What factories and multi-site companies need beyond a single desktop:
- a self-hostable **results and program server** (the open alternative to central data-management add-ons);
- users, roles and single sign-on;
- shared program and template libraries;
- integrations with MES, QMS, ERP, PLM and SPC systems.

Everything stays local-first: the server is optional and self-hosted, and the desktop works fully without it.

## Scope

**In:**
- Self-hosted server: results catalog, project and program store, template library
- Multi-user access, roles mapped to principals and governance profiles
- SSO (OIDC, SAML) and directory sync (LDAP)
- Sync between desktops and the server, with offline operation and conflict handling
- Integration connectors and an outbound event API (webhooks): MES/QMS (nonconformance, holds), ERP (part and lot data), PLM (Teamcenter, Windchill, 3DEXPERIENCE: CAD/PMI pull and results push), SPC servers
- Dashboards across sites
- Deployment: container images, Kubernetes manifests, backup and restore, upgrades

**Out:**
- Local results catalog (RPT.CATALOG; ENT extends it)
- Hosted verifier (TRUST)
- Commercial hosting services (business documents)

## Requirements

- **REQ-ENT-001** The project shall provide a self-hostable server that stores results, projects, programs and templates for many users. Every desktop workflow shall keep working while the server is unreachable.
- **REQ-ENT-002** The server shall authenticate users through OIDC or SAML and map them to principals and roles. Principals shall come only from the authenticated channel (CONSTITUTION §3).
- **REQ-ENT-003** When a desktop reconnects, it shall synchronise its journals and results with the server without losing or duplicating any committed entry, and shall surface conflicts for human resolution.
- **REQ-ENT-004** The server shall expose a versioned outbound event API (webhooks) for result release, nonconformance and program release, with a published schema.
- **REQ-ENT-005** The project shall provide integration connectors for at least one MES/QMS, one PLM and one SPC system, each as an installable plugin with a conformance test.
- **REQ-ENT-006** The server shall ship as container images with documented backup, restore and zero-data-loss upgrade procedures.

## Interfaces

- **Offers:** the server, `server.*` and `integrate.*` commands, the event API, connector SDK.
- **Consumes:** CORE (journal, principals), RPT (catalog), TRUST (signatures), AUTO (API generation).

## Decomposition hints

SERVER (results, projects and program store), IAM (users, roles, SSO, LDAP), SYNC (desktop–server sync and conflicts), EVENTS (webhooks and event schema), CONNECT (MES/QMS/ERP/PLM/SPC connectors), DASH (cross-site dashboards), OPS (containers, backup, upgrades).

## Open questions

- PLM connectors depend on vendor APIs and terms [unverified]. Pick the first PLM target with a design partner.

## Split rationale

Divided by **ownership of server-side responsibility**: one child owns the server's stored state (SRV), one owns who may call it (IAM), one owns movement of data between desktops and the server (SYNC), one owns the outbound event channel (EVENTS), one owns vendor-specific integrations as plugins (CONNECT), one owns cross-site views (DASH), and one owns running the server (OPS). Each REQ-ENT requirement lands in one child, except REQ-ENT-001, whose storage half sits in SRV and DASH and whose offline half sits in SYNC. Children depend on specific CORE, RPT, TRUST, AUTO, ECO and IO nodes rather than whole domains, to keep the dependency graph acyclic.

Alternatives considered:
- **By external system** (MES, ERP, PLM, SPC as top-level children): rejected; it scatters identity, sync and events across every child.
- **Server vs. desktop side**: rejected; sync and identity span both sides and would be split in two.
- **Merging DASH into SRV and EVENTS into CONNECT**: rejected; dashboards reuse a desktop feature with distinct access rules, and the event API is a published contract that third parties use without any connector.

The principal mapping for server callers needs a maintainer choice and is raised as D-030 (related to D-012).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ENT.SRV | Server core: results catalog, project and program store, template library | REQ-ENT-001 | |
| ENT.IAM | Identity and access: SSO, directory sync, roles and principal mapping | REQ-ENT-002 | |
| ENT.SYNC | Desktop-server synchronisation, offline operation and conflicts | REQ-ENT-001, REQ-ENT-003 | |
| ENT.EVENTS | Outbound event API: webhooks and published event schema | REQ-ENT-004 | |
| ENT.CONNECT | Integration connectors: SDK, MES/QMS, ERP, PLM and SPC | REQ-ENT-005 | |
| ENT.DASH | Cross-site dashboards on the server | REQ-ENT-001 | |
| ENT.OPS | Server deployment: containers, backup, restore and upgrades | REQ-ENT-006 | |
