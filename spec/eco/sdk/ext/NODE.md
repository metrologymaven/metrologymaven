---
id: ECO.SDK.EXT
title: Extension-point catalogue and plugin registration
level: 3
parent: ECO.SDK
kind: module
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ECO.SDK-003, REQ-ECO.SDK-008]
covers: []
depends_on: [ECO.SDK.API, CORE.REG, AUTO.SURF]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.SDK.EXT: Extension-point catalogue and plugin registration

## Purpose

The list of things a plugin may add to the platform, and the single registration path for each: commands, element types, checks, algorithms, importers and exporters, report templates and panels. It also defines extension-point **profiles** (named subsets) that other domains publish for their plugin kinds, such as the industry-pack profile (IND.CONTRACT) and driver packages (HAL.HOST), and the rules that keep plugin values of record replayable and attributable.

## Scope

**In:**
- Catalogue of extension points with their SDK types and stability level
- Plugin namespacing of registered IDs (commands, element types, algorithms)
- Profiles: named subsets of extension points, defined here as a mechanism and filled by consuming domains
- Algorithm IDs for plugin algorithms carrying plugin identity and version; replay of plugin results
- Registration-time validation against the manifest (nothing registered that the manifest does not declare)

**Out** (and which node owns it instead):
- Generated CLI, MCP and Python surfaces for plugin commands (owned by `AUTO`)
- The pack profile content and pack stamping (owned by `IND.CONTRACT`); driver profile content (owned by `HAL.HOST`)
- Isolation and capability enforcement at run time (owned by `ECO.SDK.ISO`)
- Manifest format (owned by `ECO.SDK.PKG`)

## Requirements

- **REQ-ECO.SDK.EXT-001** The SDK shall publish a catalogue listing every extension point with its ID, SDK type, stability level and the side-effect classes a registration under it may declare.
- **REQ-ECO.SDK.EXT-002** Every command a plugin registers shall have an ID whose domain segment is the plugin's registered namespace, and shall appear in the registry manifest with the plugin identity and version.
- **REQ-ECO.SDK.EXT-003** If a plugin registers anything its manifest does not declare, or uses an extension point outside the profile its manifest names, then the host shall refuse the registration and report the item and profile.
- **REQ-ECO.SDK.EXT-004** Every algorithm a plugin registers shall have an algorithm ID of the form required by CONSTITUTION §3 that includes the plugin namespace, and every value of record it produces shall carry that ID and the plugin version.
- **REQ-ECO.SDK.EXT-005** When a journal containing plugin commands is replayed with the same plugin version, the replay outcome shall follow CONSTITUTION §4 as for built-in commands.
- **REQ-ECO.SDK.EXT-006** A plugin shall not register a command of side-effect class M1, S1, W3 or A0.

## Interfaces

- **Offers:** extension-point catalogue; registration API; profile mechanism (consumed by IND.CONTRACT and HAL.HOST); plugin algorithm-ID rule.
- **Consumes:** CORE.REG registration and manifest; AUTO.SURF generation of surfaces for plugin commands; ECO.SDK.API types.

## Decomposition hints

CAT (catalogue), NS (namespacing), PROFILE (profiles), CMD, ELEM, CHECK, ALGO, FORMAT, TEMPLATE, PANEL (one child per extension-point family), REPLAY.

## Open questions

- How a plugin namespace is reserved (first come, maintainer-assigned, or derived from the signing key) needs a rule; it affects MCP tool names (`<namespace>_<verb>_<object>`, 64-character limit).
- REQ-006 forbids plugins from registering M1 commands; HAL driver packages register device commands. Whether driver packages may register D1 commands, and how driver motion is routed through the approval station, is HAL's to define within this rule (cross-domain).
- Panels depend on D-005 (provisional egui); a plugin panel API is the riskiest extension point for stability.

## Children

_None yet: status `open`._
