---
id: ENT.DASH
title: Cross-site dashboards on the server
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ENT-001]
covers: []
depends_on: [RPT.STATS.DASH, RPT.STATS.DSET, RPT.MODEL.FMT, ENT.SRV, ENT.IAM]
cli: [fid server *-site-dashboard]
mcp: [server_*_site_dashboard]
journal: [server.*_site_dashboard@*]
updated: 2026-11-16
---

# ENT.DASH: Cross-site dashboards on the server

## Purpose

Lets a quality team see statistics across sites and desktops from the server: the `RPT.STATS.DASH` dashboard definitions, evaluated against the server catalog with site filters and role-based visibility. It holds no numbers of its own. It is a separate unit because it adds site scoping, server-side evaluation and access control on top of a desktop feature, without changing that feature.

## Scope

**In:**
- Server-side dashboard definitions using the `RPT.STATS.DASH` definition format, with site and organisation scope
- Evaluation of tiles against the server catalog through the kernel statistics
- Visibility: a viewer sees only sites their role allows
- Opening a server dashboard from a desktop app or the self-hosted web build
- Staleness display: last sync time per contributing site

**Out** (and which node owns it instead):
- Dashboard definition format, tiles and statistics (`RPT.STATS`)
- Server catalog and queries (`ENT.SRV`)
- Roles and site scopes (`ENT.IAM`)
- Chart drawing and dashboard UI (`VIZ`, `RPT.STATS.DASH`)

## Requirements

- **REQ-ENT.DASH-001** A server dashboard shall use the `RPT.STATS.DASH` definition format plus a site scope, and shall store no computed values.
- **REQ-ENT.DASH-002** When a server dashboard is opened, the server shall compute every tile through the deterministic kernel from catalog entries within the viewer's allowed sites only.
- **REQ-ENT.DASH-003** Every number shown shall be produced by a kernel formatter through `RPT.MODEL.FMT`, and the same data set shall give the same numbers on the server and on a desktop.
- **REQ-ENT.DASH-004** Each dashboard shall show, per contributing site, the time of the last successful sync, and a site not synced within a configured interval shall be flagged as stale.
- **REQ-ENT.DASH-005** Saving a server dashboard shall be a journaled W2 server command; opening and listing shall be R1 and R0.

## Interfaces

- **Offers:** `server.*_site_dashboard` commands; dashboard results in the shared result envelope.
- **Consumes:** dashboard definitions and tiles (`RPT.STATS.DASH`), data sets (`RPT.STATS.DSET`), formatters (`RPT.MODEL.FMT`), catalog (`ENT.SRV`), site scopes (`ENT.IAM`).

## Decomposition hints

DEF (server dashboard definition and scope), EVAL (server-side evaluation and visibility), STALE (per-site freshness).

## Open questions

- Whether a desktop can open a server dashboard and keep a cached copy offline is not decided here; caching computed numbers would conflict with REQ-ENT.DASH-001 unless the cache is labelled.

## Children

_None yet: status `open`._
