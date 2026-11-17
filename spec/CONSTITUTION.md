# Constitution

Every node in `spec/` obeys these rules. Change them only through a decision record accepted by the steering group (see GOVERNANCE.md).

Sources: condensed from Design Baseline v3.1 (`../docs/concept/00-design-baseline.md`) and the [ArtCraft design lessons](../research/artcraft-design-lessons.md). Commercial, funding and expert-network material from the baseline is deliberately left out. Where this file and the baseline disagree, this file governs the open-source project.

## 1. Principles

| # | Principle |
|---|---|
| C1 | **Deterministic kernel.** Every value of record (fits, evaluations, uncertainties, decisions) is computed by a pure-Rust, f64, CPU kernel with no I/O. The command bus is its only caller. LLMs never compute, edit or override a value of record. |
| C2 | **The GPU is for display only.** No value of record comes from the GPU. Picks and lassos are resolved on the CPU into index sets or semantic queries before they are journaled. GPU output is tested against a CPU reference. |
| C3 | **One command registry, no side doors.** Every change to document, record or configuration state is a registry command. GUI menus, CLI, MCP tools, Python bindings, the control channel and documentation are **generated** from the registry manifest. |
| C4 | **Record everything.** Every command is written to a semantic journal that can be replayed, diffed and turned into a program. Screenshots and video capture are commands too. |
| C5 | **Humans adopt agent work.** Agent-authored plans, filters, alignments and decision-rule choices are provenance-tagged. A human must adopt them before results produced under them are released. |
| C6 | **Untrusted text is data.** Every string that comes from content is typed as untrusted and never treated as instructions. |
| C7 | **Local first.** Everything runs offline, with no account. Telemetry is off unless the user opts in. |
| C8 | **Verification is free.** Result formats and the verifier are open and free under every licensing outcome. |
| C9 | **Familiar workflows, original product.** Match incumbents' workflows and vocabulary to lower switching costs. Never copy their code, text, icons or trade dress. |

## 2. Architecture

- **Family of focused apps on one shared core** (D-004): Inspect (scan inspection), Probe (CMM programming), Network (large-volume) and Report (reporting and SPC). All share the kernel, data model, registry, journal, viewer, HAL, one project format and one `fid` CLI/MCP surface. Shared libraries contain no app policy.
- **Engine first, thin UI.** Pure-data document model + command engine + UI. Crate layering is enforced at build time: the kernel depends on nothing above it.
- **Languages:** Rust for the engine, kernel, apps and CLI. **wgpu** for rendering. Native GUI with no webview, plus a WASM build of the viewer and apps for the web. Python for user scripting (an out-of-process host). TypeScript only for an MCP App host shim if one is needed.
- **UI toolkit** (D-005, provisional): egui + winit + wgpu. It is confirmed only after prototype spikes on the element tree (10k items), the characteristic table and the report designer.
- **Binaries:** `fid` (CLI, including `fid mcp`), one desktop app per focused app, and `fid-verify` (verifier, never licence-checked).
- **Crates** use the `fid-` prefix. Dependencies of the kernel graph are on an allowlist.

## 3. Commands, CLI, MCP and journal names

- Registry command ID: `domain.verb_object@major`, e.g. `align.run_bestfit@1`. Algorithm ID: `<module>.<feature>.<method>@<semver>`.
- **MCP tool name:** drop `@major` and replace dots with underscores (`align_run_bestfit`). Pattern `[a-z0-9_]`, at most 64 characters. A breaking change adds `_vN`. No hand-written aliases.
- **CLI:** `fid <domain> <verb-object> [args]`, e.g. `fid align run-bestfit`. Every command supports `--json` output and dry-run where meaningful.
- **Journal event:** the command ID plus a result digest. One event per committed command.
- In node frontmatter, `cli:`, `mcp:` and `journal:` hold these names, or patterns with `*` above leaf level.

### Side-effect classes

Every command declares one class. The class decides how it is projected to MCP.

| Code | Class | MCP projection |
|---|---|---|
| R0 | read | exposed |
| R1 | compute (pure, scratch result) | exposed |
| W1 | mutate draft / agent branch | named tool |
| W2 | mutate working document (journaled) | named tool; agents write to an agent branch |
| W3 | mutate controlled record (release, publish) | human-only request |
| X1 | egress (data leaves the machine) | request only |
| I1 | simulator I/O | exposed |
| D1 | device I/O, energized but no motion | request only |
| M1 | physical motion | **never on MCP**; approval station only |
| S1 | signing | human only, out of band |
| A0 | admin and safety configuration | **never on MCP** |

Agents never perform M1, S1, W3 or A0, and never adopt their own proposals. Agents never pass raw file paths; they get handles to approved import and export roots and cannot widen those roots.

**Motion (D-008):**
- Agents may not file run requests over MCP. Their only motion-related tools are `run:hold` and read-only status.
- A program that an agent authored, in whole or in part, needs release by a human other than the requester. The Regulated profile requires this for every program.
- Otherwise the same person may release and approve, as two distinct acts, with mandatory simulation and a setup run.

### 3.1 Class assignments for common operations (D-007)

| Operation | Class |
|---|---|
| Write an export file into an approved **local** export root | W1, `fs_effect: write_root`. X1 only when the destination leaves the machine. |
| Install, enable, update or remove a plugin or industry pack | A0 (never on MCP). Listing them is R0. |
| Cancel a job | W1 for the requester's own jobs. Cancelling another principal's job needs a human. |
| Live alarm sound or visual | Not a command (UI effect). Acknowledging an alarm is W2. |
| MCP "describe, then invoke by command ID" dispatcher | Allowed for R0/R1 only. It is generated, so it is not an alias. |
| Single-verb CLI command where the domain is the verb (e.g. `fid doctor`) | Allowed when generated from the registry |

## 4. Numerics, determinism and replay

- **Units:** SI metres inside the kernel (f64). Authored decimals keep their value and unit, and each conversion is a single rounding that is journaled.
- **KNP-1 numeric profile** for the kernel:
  - no fused multiply-add (FMA);
  - transcendental functions only through the `fid-num` facade;
  - no runtime SIMD dispatch;
  - fixed-order reductions;
  - no std HashMap or HashSet;
  - total-order min/max;
  - a seeded, counter-based RNG;
  - NaN is an error.
- **Determinism classes:** `D0` portable bit-exact · `D1` per-platform bit-exact · `D2` seeded · `D3` tolerance-convergent · `N` observed.
- **Replay outcomes:** `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`. Any change to a conformance decision is `DIVERGENT`.
- **Replay modes:** exact · re-targeted (same program, new data) · verifier · dry-run/simulation · cross-version diff.

## 5. Data model and results

- **QIF 3.0 (ISO 23952)** is the native model and exchange format. Extensions go only through QIF's sanctioned hooks.
- **Project container:** Part → Revision → Plan → Run.
- Every entity has a stable ID: imported IDs are preserved, new IDs are UUIDv7, and derived IDs are UUIDv5.
- **ResultRecord** (minimum fields):
  - characteristic;
  - measurand;
  - `evaluation_basis` (`POINTS | FEATURES | RESULTS_ONLY | DIRECT_READING`);
  - value with unit;
  - uncertainty: UEC-0..3, where UEC-0 means "not evaluated" and is never fabricated;
  - decision with rule ID and outcome (`CONFORMS | NONCONFORMS | INDETERMINATE | NOT_DECIDABLE`);
  - spec context: standard, edition and defaults profile;
  - algorithm ID and version;
  - kernel version;
  - input digests;
  - journal entry hash.
- **Standards:** cite designators, editions and clause numbers only. Never store or serve standards text. Never send it to an external LLM.

## 6. Recording layers

1. **Semantic journal:** an append-only, hash-chained record of commands (RFC 8785 canonical JSON + SHA-256).
2. **Presentation track:** about a 10 Hz view track, plus optional video segments on the journal clock, and screenshots as commands.
3. **Instrument I/O transcripts** at the HAL boundary.

## 7. Phases

Every node carries a `phase:` tag, which the steering group may change only through a decision.

A phase means **when work starts** (D-006). A parent's phase is the earliest phase of any of its children, and `scope.py` warns when a child would start before its parent.

| Phase | Theme |
|---|---|
| P0 | Foundations: numerics, least-squares and minimum-zone fits, registry, CLI and stdio MCP, journal, QIF subset, verifier |
| P1a | First usable product: transcription reports, prismatic GD&T subset (ASME), minimal GUI and viewer |
| P1b | Scan-to-CAD inspection, ISO GPS dialect, CAD + PMI, SPC, Python, report designer |
| P2 | Live devices and CMM programming: I++ client, simulator, approval station, DMIS import |
| P3 | Form and texture, live portables, large-volume networks, auto-path |
| P4 | Ecosystem: OPC UA, MTConnect, marketplace, program conversion from incumbents |
| later | Not yet scheduled |

## 8. Engineering rules (binding on implementation; specs must not contradict them)

- **Never crash.** No panic and no `unsafe` in non-test code (kernel: no exceptions; other crates: named exceptions only). Deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented` and `unreachable`. All input is hostile; parsers are bounded and fail closed.
- **Clean room.** Public specs and observed behaviour only. No copied code, strings, icons or assets. `ATTRIBUTION.md` lists every third-party asset.
- **Licence allowlist** of SPDX IDs for dependencies (to be set by D-001).
- **Corpora.** Format readers and fits are tested against real-file and reference datasets, and their fidelity is published as numbers.
- **Honest status.** Docs state gaps plainly. Coverage and parity reports are generated, never hand-edited.
- **Repository hygiene.** `main` is PR-only, CI changes go in their own PRs, and agent rules follow AGENTS.md.
