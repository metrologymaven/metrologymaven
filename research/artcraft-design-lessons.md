# Design lessons from the ArtCraft / "Craft" family

Researched 2026-11-16 from the public repositories under `github.com/storytold`: `artcraft`, `photocraft`, `craft-libs`, `craft-contrib` (house rules and RFCs), `craft-anything`, and the profile listing. GitHub star counts differ between snapshots, so they are not used as evidence. All quotations are short. Everything else is paraphrased.

## What the family is

One organisation ships about 80 repositories. They include many focused, clean-room Rust desktop apps, each modelled on one incumbent: PhotoCraft (Photoshop), LightCraft (Lightroom), FilmCraft, EffectCraft, PdfCraft, WordCraft, GridCraft, SoundCraft and others. These apps sit on top of three shared repositories:

| Repo | Role |
|---|---|
| `craft-libs` | One Cargo workspace of reusable crates (formats, UI widgets, IPC, settings, atomic writes). Apps pin it by git tag. |
| `craft-contrib` | House rules, standards and RFCs that every repo follows |
| `craft-common-actions` | Shared CI that every repo calls, including the check that house rules haven't been tampered with |

## Core design choices

### Architecture

1. **Many focused apps on one shared library repo.** Each app copies one incumbent's job. Shared code lives in `craft-libs`. Libraries contain **no app policy**: mapping onto a document, editing and export decisions "stay in the app". A library that outgrows the family can be promoted into its own project.
2. **Engine first, thin UI.** PhotoCraft has a pure-data document model and a command engine, with an egui UI on top. **Layering is enforced at build time** across 24 crates.
3. **One command registry backs everything.** PhotoCraft has 500+ commands behind every menu item, tool and dialog. The UI, CLI, control channel and MCP all call the same command IDs.
4. **GPU rendering, CPU reference.** A wgpu compositor (Metal, Vulkan, DX12, WebGPU) is tested against a **CPU compositor that serves as the oracle**.
5. **Native Rust only.** No Electron, no webview, no handwritten JS/TS in the native apps. The same engine and UI compile to **WASM for a self-hostable web build**.
6. **Offline and local.** No account, no cloud and no telemetry for core features.
7. **Non-destructive and cheap undo.** Live adjustment layers and copy-on-write tiles give full history.

### Agent-ready by construction

8. **The CLI has an `mcp` subcommand** with three modes:
   - `--bridge`: drive the running app;
   - `--headless`: an in-process session with no window;
   - default: use the open app, or follow a setting for what to do when it is closed.
9. **A loopback control protocol** carries JSON lines over 127.0.0.1 and is used to:
   - inspect UI state (`ui.inspect`);
   - list commands with an enabled flag and the reason when disabled;
   - run any command (`engine.execute`);
   - drive pointer and keyboard input;
   - **take screenshots** (`ui.screenshot`);
   - **record and play back actions**;
   - list and cancel long jobs.

   Authentication is a challenge with HMAC proofs from a token file, so the token is never sent. The token file is created with user-only permissions.
10. **Agents cannot widen their own access.** File access goes through configured read and write roots. Commands that take paths are refused on the control channel, and automation cannot change the root folders.
11. **Timeouts are honest.** "Not run" is safe to resend. "No reply" means the command may still apply.

### Proving parity, honestly

12. **The parity checklist is generated, not hand-written.** `cargo xtask parity` compares the incumbent's menu catalog with the live command registry and prints live/total for each menu (664 of 664 for PhotoCraft). The output is marked "do not edit".
13. **Real-file test corpora** are fetched by `xtask`. Round-trip fidelity is published as numbers (for example, a share of a third-party test suite that re-renders identically).
14. **READMEs state gaps** and alpha status plainly.
15. **Familiar by design.** Menus and shortcuts follow the incumbent, which lowers the cost of switching.

### Safety and robustness rules (house-rules-rust)

16. **Never crash.** No panic and no `unsafe` in non-test code. `unwrap`, `expect`, `panic`, `todo`, `unimplemented` and `unreachable` are denied as lints. Input is hostile; parsing is **bounded and fail-closed**. Every crash fix comes with a test that panicked before the fix.
17. **Clean room.** Only public specs and observed behaviour. No copied code, strings or assets.
18. **Licence allowlist** of SPDX IDs, plus `ATTRIBUTION.md` for every asset.

### Community machinery

19. **House rules are synced, not linked.** The canonical text lives in `craft-contrib`. Each repo's `AGENTS.md` carries a generated block with the source revision and a digest. Shared CI fails if the block is edited and warns if it is stale. Size budgets: 3 KB for the core block and a 10 KB soft limit for `AGENTS.md`, because agents may not follow links.
20. **`CLAUDE.md` contains just `@AGENTS.md`.** This avoids symlinks on Windows.
21. **Agent rules (RFC-0006):**
   - the human who runs an agent is accountable for what it does;
   - check, then claim, before coding;
   - read a definition before using it, and treat triage notes as hypotheses;
   - narrow fixes, each with a test that fails without the fix;
   - review the diff before opening a PR;
   - close your PR if a better one exists;
   - report only what actually ran;
   - no agent attribution in PRs;
   - stop and ask only for high-stakes actions (licensing, publishing, releases, secrets, money, destructive git).
22. **PR timelines:** respond within 7 days, reminder at 14, closure possible at 30. The clock runs only while the author is the blocker. Bot findings don't start it.
23. **`main` is PR-only, for everyone.** CI changes never ride along in feature PRs.
24. **RFC process:** a numbered file in a PR. A maintainer starts a 3-day comment period (7 days for adding or removing an app, or changing the process). Approval must come from a maintainer other than the author, with no unresolved objection, and every objection must say what would resolve it.
25. **Bot review on every PR:** CodeRabbit and SonarCloud.
26. **License:** MIT OR Apache-2.0. The trademark is excluded from the license.

## What we adopt, adapt or reject

| # | Choice | Verdict for Fiducial | Where |
|---|---|---|---|
| 1 | Focused apps on shared libs | **Adopt.** Separate apps for scan inspection, CMM programming, large-volume networks and reporting, all on one shared kernel and libraries. | CONSTITUTION §2, D-004 |
| 2–3 | Engine first; one registry backs everything | **Adopt** (already baseline P4). Add build-time layering enforcement. | CONSTITUTION §2–3 |
| 4 | GPU tested against a CPU oracle | **Adopt and strengthen.** For us the CPU kernel is the only source of values of record, not just an oracle. | CONSTITUTION §1 |
| 5 | Rust only, WASM for web | **Adopt for apps.** TypeScript is limited to unavoidable host shims (pending D-005). | CONSTITUTION §2, D-005 |
| 6 | Offline, no telemetry | **Adopt** | REQ-FID-017 |
| 8–11 | CLI `mcp` modes, control protocol, screenshots, recording, roots | **Adopt** as AUTO and VIZ requirements | spec/auto, spec/viz |
| 12 | Generated parity checklist | **Adopt.** Seeds plus `scope.py` coverage now; a registry-generated parity report once code exists. | ECO |
| 13 | Real-file corpora, published fidelity numbers | **Adopt** for format round-trips and fit accuracy (NIST/PTB data) | IO, UNC |
| 15 | Familiar by design | **Adapt.** Copy workflows and vocabulary, never trade dress or text. | VIZ, CONSTITUTION §8 |
| 16–18 | Never crash, clean room, licence allowlist | **Adopt** | CONSTITUTION §8 |
| 19–25 | Synced house rules, agent rules, timelines, RFCs, bots | **Adopt.** A `fid-contrib` repo once there is more than one repo; until then they live here. | AGENTS.md, CONTRIBUTING.md, GOVERNANCE.md, ECO |
| 26 | MIT/Apache, trademark excluded | **Proposed** in D-001 | D-001 |
| — | Clean-room reimplementation of a named incumbent | **Reject as a framing.** We target incumbents' workflows and use their features as a coverage checklist, but we never name our products after them or imitate their look. | CONSTITUTION §8 |
