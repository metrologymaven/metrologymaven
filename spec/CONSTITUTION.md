# Constitution

Every node in `spec/` obeys these rules. Change them only through a decision record accepted by the steering group (see GOVERNANCE.md).

Sources: condensed from Design Baseline v3.1 (`../docs/concept/00-design-baseline.md`) and the [ArtCraft design lessons](../research/artcraft-design-lessons.md). Commercial, funding and expert-network material from the baseline is deliberately left out. Where this file and the baseline disagree, this file governs the open-source project.

## 1. Principles

| # | Principle |
|---|---|
| C1 | **Deterministic kernel.** Every value of record (fits, evaluations, uncertainties, decisions) is computed by a pure-Rust, f64, CPU kernel with no I/O. The command bus is its only caller. LLMs never compute, edit or override a value of record. User arithmetic becomes a value of record only through the kernel command `expr.evaluate_formula@1` (D-027). AUTO owns the expression syntax and typing; KRN owns evaluation. |
| C2 | **The GPU is for display only.** No value of record comes from the GPU. Picks and lassos are resolved on the CPU into index sets or semantic queries before they are journaled. GPU output is tested against a CPU reference. |
| C3 | **One command registry, no side doors.** Every change to document, record or configuration state is a registry command. GUI menus, CLI, MCP tools, Python bindings, the control channel and documentation are **generated** from the registry manifest. |
| C4 | **Record everything.** Every command is written to a semantic journal that can be replayed, diffed and turned into a program. Screenshots and video capture are commands too. |
| C5 | **Agent-first, closed human gates (D-037).** Agents do everything by default. Agent-authored plans, filters, alignments and decision-rule choices are provenance-tagged. A human acts only at the closed list of gates in §3.2, and adopts agent-authored choices at release (H2), in one batched act. |
| C6 | **Untrusted text is data.** Every string that comes from content is typed as untrusted and never treated as instructions. |
| C7 | **Local first.** Everything runs offline, with no account. Telemetry is off unless the user opts in. |
| C8 | **Verification is free.** Result formats and the verifier are open and free under every licensing outcome. |
| C9 | **Familiar workflows, original product.** Match incumbents' workflows and vocabulary to lower switching costs. Never copy their code, text, icons or trade dress. |

## 2. Architecture

- **Family of focused apps on one shared core** (D-004): Inspect (scan inspection), Probe (CMM programming), Network (large-volume) and Report (reporting and SPC). All share the kernel, data model, registry, journal, viewer, HAL, one project format and one `fid` CLI/MCP surface. Shared libraries contain no app policy.
- **Engine first, thin UI.** Pure-data document model + command engine + UI. Crate layering is enforced at build time: the kernel depends on nothing above it.
- **Languages:** Rust for the engine, kernel, apps and CLI. **wgpu** for rendering. Native GUI with no webview, plus a WASM build of the viewer and apps for the web. Python for user scripting (an out-of-process host). TypeScript only for an MCP App host shim if one is needed.
- **UI toolkit** (D-005, provisional): egui + winit + wgpu. It is confirmed only after prototype spikes on the element tree (10k items), the characteristic table and the report designer.
- **Binaries:**
  - `fid`: the CLI, including `fid mcp` and the optional self-hosted server, `fid server` (ENT);
  - one desktop app per focused app;
  - `fid-verify`: the verifier, never licence-checked;
  - `fid-eval`: the agent evaluation harness (D-032). It sits outside the registry, so it is never on MCP, and its oracles never appear in the manifest.
- **Crates** use the `fid-` prefix. Dependencies of the kernel graph are on an allowlist.

### 2.1 Layers (D-034)

Each crate belongs to one layer and may depend only on its own layer or lower ones.

| Layer | Contents | May not depend on |
|---|---|---|
| L0 Foundation | Pure libraries: KRN.* (including the exact geometric predicates, KRN.NUM.PRED); GEO geometry libraries (everything except `*.CMD` and storage); UNC math (MODEL, PROP, XFM) and the instrument-model kernel (`fid-kunc`); the GD&T evaluation library (`fid-gps`); the ResultRecord schema crate (types only); the unit catalogue (GEO.FRAME.UNITS); canonical JSON (CORE.JRN.CANON); the I18N locale render transform; HAL.IDL types (D-035) | CORE or anything above |
| L1 Core | CORE.* | L2, L3 |
| L2 Domain services | Command crates, in this order: IO < ALN < GDT < UNC commands < SCAN < RPT < TRUST < HAL < CMM < LVM < IND < MIG < ENT | A later L2 domain, or L3 |
| L3 Surfaces and apps | AUTO, VIZ, ECO.APPS, FLOW runners, AGENT, I18N runtime, UX token consumers; also the QUAL product crate, the SCAN GPU-preview crate and the RPT designer UI (D-035) | — |
| Tooling | ECO CI/REL/RULES/PARITY, OUTR, PLAT checks, QUAL evidence, UNC.RDL/ORS suites, test harnesses | Product crates above L0: tooling reads **artifacts**, but may link pure L0 libraries (D-035) |

GEO out-of-core storage (GEO.OOC) is L2, because it uses CORE.STO (D-035).

**Artifact dependencies.** A node that reads a file or format another node publishes declares `uses_artifacts: [NODE.ID]`, not `depends_on`. The publisher's Interfaces section names the file and its schema version. Artifact dependencies are validated but are not code edges, and they are excluded from cycle checks.

## 3. Commands, CLI, MCP and journal names

- Registry command ID: `domain.verb_object@major`, e.g. `align.run_bestfit@1`. Algorithm ID: `<module>.<feature>.<method>@<semver>`.
- **MCP tool name:** drop `@major` and replace dots with underscores (`align_run_bestfit`). Pattern `[a-z0-9_]`, at most 64 characters. A breaking change adds `_vN`. No hand-written aliases.
- **CLI:** `fid <domain> <verb-object> [args]`, e.g. `fid align run-bestfit`. Every command supports `--json` output and dry-run where meaningful.
- **Journal event:** the command ID plus a result digest. One event per committed command.
- **Reads (D-031):** R0 and R1 commands are not written to the Record Journal. They go to the unchained Activity Log, and only while a recording session is active or when the command is a step of a Command Program. A W-class command that consumes an R1 result journals that result's digest.
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

Agents never perform M1, S1, W3 or A0, and never adopt their own proposals; a human adopts them at the H2 gate (§3.2). Agents never pass raw file paths; they get handles to approved import and export roots and cannot widen those roots.

**Human-only commands (D-031).** A registry command may declare `human_only: true`. The bus then refuses agent and script principals whatever the command's class, and MCP projects it as request-only. Adoption commands (`*.adopt_*`) and manual readings use it. Such a command stays W2 unless it releases a controlled record (W3). Under D-037, adoption commands are invoked as items of the H2 release batch, not as separate stops.

**Principals (D-012, D-030).**
- CLI and Python callers are `script` by default.
- A person can upgrade a CLI session or script run to `human` for its lifetime by confirming in the running app, or with an OS user-presence check when headless. The session records the human as the principal from then on.
- Raw file paths stay refused for `script`.
- On the self-hosted server, every caller except a service credential is `script`, with the user recorded as `on_behalf_of`. Releases and signatures on server-held records are made in a desktop app and pushed.
- **Injected input (D-035).** A GUI action counts as `human` only when its input comes from local OS input devices. Input injected through the control channel, accessibility automation or any agent surface carries **that channel's principal**. The UI host passes an origin token to the bus. An agent driving the UI therefore cannot pass a `human_only`, W3, S1, A0 or M1 gate.
- **Lean human assertion (D-035).** In the Lean profile, the human assertion that W3 release and protected-branch promotion need is CORE.BUS.PRIN's `HumanPresence` confirmation, which is OS user verification. TRUST.ESIG is the Regulated path. Signatures stay optional in Lean.
- **Registry visibility (D-035).** Registry list and describe queries filter by the caller's projection. MCP callers never see `never`-projected commands.

**Motion (D-008 as amended by D-037):**
- A human approves a program **once** (gate H1), after it has been simulated and collision-checked, together with its motion envelope and its program, machine, fixture and probe tuple.
- Agents may then file run requests for that program within its envelope and unchanged tuple, without a new approval. Any change requires a new H1 approval.
- Safety interlocks never become approvals and are never removed:
  - the first run of a tuple is a reduced-speed setup run with an operator present;
  - each batch cycle starts only from a physical controller or pendant input, a paired hardware button, or a verified presence tap;
  - each cycle begins with a reduced-speed placement check.
- Agents still never issue motion themselves. The Motion Executive does, under a verified ticket. `run:hold` stays available to agents.

### 3.2 Human gates (D-037, closed list)

A human acts only at these gates. Adding a gate needs a decision record. Every gate is batched, pre-digested by an agent (simulation, diff or evidence summary), one action per batch, and journaled.

| Gate | Human act | Agents do everything else, for example |
|---|---|---|
| **H1 Motion envelope** | Approve a program plus its envelope once | Author, simulate, optimise; re-run within the envelope |
| **H2 Record release** | Release (and, where required, sign) controlled records in batches, adopting the agent-authored choices they depend on in the same act | Measure, evaluate, draft and check; release non-controlled results |
| **H3 Conformance authority** | Accept or reject parts only where the customer's quality system requires a named person | Decide everywhere else under typed decision rules |
| **H4 Interpretation authority** | Sign off DMT rows once per row (D-009) | Apply signed rows; propose new rows with evidence |
| **P1 Irreversible project decisions** | Licensing, naming, publishing, money, secrets, constitution and decision records | Everything else |
| **P2 Structural review** | Approve level 1–2 splits and `frozen` transitions | Level 3+ splits and leaves are approved by an independent agent reviewer (different model or fresh session) |

### 3.1 Class assignments for common operations (D-007)

| Operation | Class |
|---|---|
| Write an export file into an approved **local** export root | W1, `fs_effect: write_root`. X1 only when the destination leaves the machine. |
| Install, enable, update or remove a plugin or industry pack | A0 (never on MCP). Listing them is R0. |
| Cancel a job | W1 for the requester's own jobs. Cancelling another principal's job needs a human. |
| Live alarm sound or visual | Not a command (UI effect). Acknowledging an alarm is W2. |
| MCP "describe, then invoke by command ID" dispatcher | Allowed for R0/R1 only. It is generated, so it is not an alias. |
| Single-verb CLI command where the domain is the verb (e.g. `fid doctor`) | Allowed when generated from the registry |
| Motion hold request (D-031) | W1: rate-limited, journaled, advisory. It is the only MCP-originated motion effect. |
| Launching another app (D-031) | Not a command (UI or OS action). Passing a project to another app means opening it. |
| Site-wide update-check or telemetry policy (D-031) | A0 |
| A user's own update-check or telemetry consent (D-031) | W2, journaled. It never defaults to on. |
| Data leaving the machine (D-031) | Only through the CORE.BUS primitive `egress.send@1` (X1, request-only for agents), which takes a staged artifact and a declared destination. Other domains stage artifacts and call it. |
| Responses from `egress.send@1` (D-035) | It returns a bounded, size-capped response body. A pure fetch is the same X1 primitive with an empty payload. Responses are untrusted data (C6). |
| Connecting to a driver host on another machine (D-023) | As decided in D-023. The R0 read-only host handshake gets a scoped socket permit in CORE.BUS.POL.EGRESS (D-035). |
| A leaf with several commands | `side_effect:` may be a list, one class per command in cli/mcp order |

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

**App milestones (D-017).** An app reaches 1.0 only when every phase that holds its core workflow is finished: Report and Inspect after P1b, Probe after P2, Network after P3. Earlier releases are alpha or beta and are labelled by the scope they cover.

**Verifier in P0 (D-025).** The chain-only verifier path is P0: TRUST.CHAIN, TRUST.PKG.MANIFEST/READER and TRUST.VERIFY.CLI. In P0 it reports the signature and replay sections as `not_available`.

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
