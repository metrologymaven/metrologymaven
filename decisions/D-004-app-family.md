---
id: D-004
title: Family of focused apps vs one integrated application
status: accepted
raised_by: founder
blocks: [ECO]
decide_by: when ECO is split
updated: 2026-11-16
---

# D-004: Family of focused apps vs one integrated application

## Question

Do we ship one large application (like PolyWorks or SpatialAnalyzer), or a family of focused apps on shared crates (like the Craft family)?

## Context

The Craft family ships one focused app per incumbent, all on a shared library repository whose libraries contain no app policy. This lets each app have its own maintainers, release pace and contributors. The concept docs assumed one `fiducial-app`.

Users do cross workflows (scan data and CMM data in one report), so apps must share the project format and open each other's projects.

## Options

| Option | For | Against |
|---|---|---|
| A. One integrated app | One install; no cross-app hand-offs | Huge surface; hard to own; slow releases |
| B. Focused apps (Inspect: scan-to-CAD · Probe: CMM programming and execution · Network: large-volume · Report: reporting and SPC) on shared crates and one project format | Clear ownership; smaller, faster-moving apps; each maps to one incumbent's buyers | Cross-app workflows need care; more packaging |
| C. B, plus a "suite" launcher that hosts several apps in one window | Both benefits | More work later |

## Recommendation

**B now, C later.** Every app opens the same project. The CLI and MCP servers are shared (`fid`), so agents see one platform.

## Decision

**Accepted (option B now, C later), 2026-11-16, founder.** A family of focused apps (Inspect, Probe, Network, Report) on shared crates and one project format; one shared `fid` CLI and MCP surface. Follow-up: ECO.APPS unblocked (status open).
