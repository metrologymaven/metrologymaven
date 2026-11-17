---
id: D-005
title: Native UI toolkit and the TypeScript boundary
status: accepted
raised_by: founder
blocks: [VIZ]
decide_by: when VIZ.SHELL is expanded
updated: 2026-11-16
---

# D-005: Native UI toolkit and the TypeScript boundary

## Question

Which Rust UI toolkit do the apps use? Is any TypeScript or webview allowed?

## Context

The Craft family is Rust-only for native apps: egui + winit + wgpu, no webview, a WASM web build, and no handwritten JS/TS. ArtCraft itself (Tauri + React) is the documented exception.

Our viewer is wgpu in any case. The open question is the panel and docking UI around it. Metrology UIs are dense: trees, tables, property grids, report designers.

## Options

| Option | For | Against |
|---|---|---|
| A. egui (immediate mode) | Proven by the Craft apps; same code on web; simple | Dense tables and complex text editing take effort; accessibility is still maturing |
| B. Slint / Iced / other retained-mode Rust UI | Retained-mode widgets; accessibility | Smaller ecosystems; less proven at this scale |
| C. Tauri/webview UI + wgpu viewport | Rich web widget ecosystem | Two languages, webview overhead, against the Craft lesson |

## Recommendation

A, with prototype spikes on the three hardest screens (element tree with 10k items, characteristic table, report designer) before committing. TypeScript only for an MCP App host shim if one is needed.

## Decision

**Accepted provisionally (option A), 2026-11-16, founder.** egui + winit + wgpu, Rust-only native apps with a WASM web build. **Confirmed only after prototype spikes** on the element tree (10k items), the characteristic table and the report designer; if a spike fails, reopen with option B. TypeScript only for an MCP App host shim if one is needed. Follow-up: CONSTITUTION §2 amended.
