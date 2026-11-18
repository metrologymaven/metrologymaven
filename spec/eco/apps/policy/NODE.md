---
id: ECO.APPS.POLICY
title: App-policy boundary and cross-app command identity
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 116
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-003]
covers: []
depends_on: []
uses_artifacts: [ECO.REPO.LAYERS, CORE.REG.MAN, CORE.REG.DEF, ECO.APPS.LIST, ECO.APPS.MANIFEST.EXPORT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.POLICY: App-policy boundary and cross-app command identity

## Purpose

Enforces the two rules that keep a family of apps from turning into several diverging products: app crates contain no measurement, fitting, evaluation or file-format logic and define no registry commands of their own; and shared crates contain no app policy (no branching on which app is running). It also proves that a command reachable in two apps is the same command in both. The checks form one repository tool (`cargo xtask apps-policy`) run by ECO.CI, plus a reviewer checklist for what a tool cannot decide. Being tooling, it reads only artifacts: `cargo metadata`, source text, `layers.toml`, `apps.toml`, per-app registry manifests and `apps-status.json`.

## Scope

**In:**
- DEFN: app-layer crates may not declare registry commands
- LEAK: shared crates may not depend on app crates or use app-identity tokens
- IDENT: same command ID implies the same definition digest (so the same parameter schema, result schema and side-effect class) in every app that enables it
- SIZE: app crates hold only the entry point
- The reviewer checklist `docs/review/app-crate-checklist.md`

**Out** (and which node owns it instead):
- Layer assignments and the general layering check (owned by `ECO.REPO.LAYERS`)
- Manifest schema and export (owned by `ECO.APPS.MANIFEST`)
- Running the check on PRs (owned by `ECO.CI.GATES` and `ECO.CI.WF`)

## Requirements

- **REQ-ECO.APPS.POLICY-001** If a crate assigned to the `app` layer in `layers.toml` has a direct dependency on the registry declaration-macro crate, or its source contains a command-declaration attribute or macro named in the check's config, then the check shall fail with `POLICY-DEFN` naming the crate and location.
- **REQ-ECO.APPS.POLICY-002** If a crate outside the `app` layer depends on a crate in the `app` layer, or its non-test source uses an app-identity token from the check's config (initially `AppId::from`, `AppId::new`, `current_app`, `FID_APP`), then the check shall fail with `POLICY-LEAK` naming the crate, file and line.
- **REQ-ECO.APPS.POLICY-003** When given one registry manifest per app and `apps-status.json`, the check shall fail with `POLICY-IDENT` for every command ID that is effective in two or more apps with differing `definition_digest`, naming the apps and digests.
- **REQ-ECO.APPS.POLICY-004** If an `app`-layer crate contains a Rust source file other than `src/main.rs`, or `src/main.rs` exceeds 60 lines, then the check shall fail with `POLICY-SIZE`, unless an entry in `apps-policy.toml` names the crate, the file and an approving maintainer.
- **REQ-ECO.APPS.POLICY-005** The project shall publish a reviewer checklist for app-crate PRs covering cases the automated checks cannot decide (numeric logic hidden in UI code, app-specific branching through configuration values, copied domain code), and the PR template shall link it.
- **REQ-ECO.APPS.POLICY-006** The check shall run offline, print findings sorted by code, crate and location, and exit 0 when there are none and 1 otherwise.

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/policy/ok/` (shared crate `fid-x` declaring command `x.do_thing@1`, app crate `fid-app-a` with only a 20-line `main.rs`), `layers.toml`, `apps.toml`, and per-app manifests where `x.do_thing@1` has the same digest in apps `a` and `b`, when `cargo xtask apps-policy --workspace fixtures/policy/ok --registry-manifests fixtures/policy/ok/manifests --apps-status fixtures/policy/ok/apps-status.json` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/policy/defn/` where `fid-app-a` depends on the declaration-macro crate and declares a command, then `POLICY-DEFN fid-app-a` is reported.
- **AC-3** (error path) Given `fixtures/policy/leak/` where shared crate `fid-x` calls `current_app()` in `src/lib.rs` line 12, then `POLICY-LEAK fid-x src/lib.rs:12`; given a shared crate depending on `fid-app-a`, then `POLICY-LEAK` naming the dependency.
- **AC-4** (error path) Given `fixtures/policy/ident/` where `x.do_thing@1` has digest `aa…` in app `a` and `bb…` in app `b`, then one `POLICY-IDENT x.do_thing@1` finding listing both.
- **AC-5** (error path) Given `fixtures/policy/size/` where `fid-app-a` has `src/fit.rs`, then `POLICY-SIZE fid-app-a src/fit.rs`; with a matching `apps-policy.toml` exception, the finding disappears.
- **AC-6** The file `docs/review/app-crate-checklist.md` exists and the PR template links it.
- **AC-7** (determinism) Two runs of AC-3 give byte-identical output.

## Design

- **Registry command(s):** none; repository tool `cargo xtask apps-policy --workspace <dir> --registry-manifests <dir> --apps-status <json> [--config apps-policy.toml]`.
- **Data structures:** `apps-policy.toml`: `macro_crates = [...]`, `declaration_markers = [...]`, `identity_tokens = [...]`, `[[exception]] crate, file, approver, reason`; inputs: `layers.toml` (ECO.REPO.LAYERS), `cargo metadata --format-version 1 --locked --offline`, one registry manifest JSON per app (`commands[].id`, `commands[].definition_digest`, produced in CI by running `fid --app <id> registry show-manifest --full` against each running app), `apps-status.json` (effective commands per app).
- **Algorithm:** DEFN: dependency edges plus a token scan of app-crate sources; LEAK: reverse edges into app crates plus a token scan of non-app, non-test sources (comments and string literals skipped by a small lexer); IDENT: build `BTreeMap<command_id, BTreeMap<app, digest>>` over effective commands and report entries with more than one distinct digest; SIZE: file listing.
- **Errors:** `POLICY-DEFN`, `POLICY-LEAK`, `POLICY-IDENT`, `POLICY-SIZE`, `POLICY-INPUT`.
- **Determinism:** D0.
- **Performance budget:** under 30 s on a 150-crate workspace [unverified until measured].
- **Crate:** `fid-xtask` (module `apps_policy`); tooling, so it reads artifacts and source text only.

## Test fixtures

Miniature workspaces, layer maps, app lists and registry manifests under `fixtures/policy/` (MIT OR Apache-2.0).

## Build prompt

> In `fid-xtask` (implementation repository), add `apps-policy --workspace <dir> --registry-manifests <dir> --apps-status <json> [--config apps-policy.toml]`. Read `layers.toml` (crate → layer; the `app` layer holds per-app binary crates), `cargo metadata --locked --offline`, one registry manifest JSON per app (`commands[].id`, `definition_digest`) and `apps-status.json` (per-app effective commands). Findings, sorted by code, crate, location: `POLICY-DEFN` (an app crate depends on a configured declaration-macro crate or its source contains a configured declaration marker); `POLICY-LEAK` (a non-app crate depends on an app crate, or its non-test source uses a configured app-identity token such as `AppId::from`, `AppId::new`, `current_app`, `FID_APP`; skip comments and string literals with a small lexer); `POLICY-IDENT` (a command ID effective in two or more apps with different `definition_digest`); `POLICY-SIZE` (an app crate has any Rust file other than `src/main.rs`, or `main.rs` over 60 lines, unless excepted in `apps-policy.toml` with crate, file, approver, reason). Exit 0 if none, 1 otherwise; offline. Also write `docs/review/app-crate-checklist.md` (hidden numeric logic in UI code, app branching through config values, copied domain code) and link it from the PR template. Tests for AC-1..AC-7 of spec node ECO.APPS.POLICY. Tooling must not link product crates (CONSTITUTION §2.1). No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- Cross-domain (CORE.REG): if CORE.REG.DEF later records the defining crate per command in the registry manifest, DEFN should also read it; until then the dependency and marker checks stand in for it. The definition digest from CORE.REG.MAN already covers parameter schema, result schema and class.
- The declaration-macro crate name is fixed by CORE.REG.DEF (open); the check reads it from config.

## Children

_Leaf: none._
