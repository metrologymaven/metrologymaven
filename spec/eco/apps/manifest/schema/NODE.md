---
id: ECO.APPS.MANIFEST.SCHEMA
title: Manifest schema, validation and start-up loader
level: 4
parent: ECO.APPS.MANIFEST
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.MANIFEST-001, REQ-ECO.APPS.MANIFEST-002, REQ-ECO.APPS.MANIFEST-003, REQ-ECO.APPS.MANIFEST-004, REQ-ECO.APPS.MANIFEST-007]
covers: []
depends_on: [CORE.REG.MAN, VIZ.SHELL.DOCK, CORE.STO.SET, RPT.MODEL]
uses_artifacts: [ECO.APPS.LIST]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.MANIFEST.SCHEMA: Manifest schema, validation and start-up loader

## Purpose

Defines `app.toml`, the one declarative file that makes an app out of the shared engine, and the loader that every app binary calls first. The loader validates the file against the registry in force, the shell's panel and layout catalogue, the setting keys, the report template IDs and the app list, and refuses to start the app if anything is unknown, listing every problem at once. The schema deliberately has no field that could change a command's side-effect class or MCP projection, so an app can only select commands, never alter them.

## Scope

**In:**
- Schema `app.toml` v1 and its documentation
- Pure validation `validate(manifest, catalogues) -> Vec<Finding>` and the start-up loader `load_manifest`
- `forbidden_classes`: classes an app declares it must never enable; validated against the registry
- Status fields (`status`, `status_scope`) and known gaps; branding placeholders
- A repository check binary `fid-app-manifest check` that runs the same validation from artifact files in CI

**Out:**
- Export of status data (owned by `ECO.APPS.MANIFEST.EXPORT`)
- Composition of several manifests (owned by `ECO.APPS.MANIFEST.COMPOSE`)
- Each app's content (owned by the per-app nodes)
- The panel catalogue, setting declarations and template IDs themselves (owned by `VIZ.SHELL.DOCK`, `CORE.STO.SET`, `RPT.MODEL`)

## Requirements

- **REQ-ECO.APPS.MANIFEST.SCHEMA-001** The schema shall consist only of the keys `schema`, `id`, `status`, `status_scope`, `known_gaps`, `namespaces`, `forbidden_classes`, `panels`, `layout`, `templates`, `defaults` and `branding`, and the loader shall reject any other key with `MAN-SCHEMA`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-002** If `namespaces`, `panels`, `layout`, `templates` or a `defaults` key names something absent from the registry manifest, panel and layout catalogue, template catalogue or setting declarations, then the loader shall refuse to start and report every unknown name with codes `MAN-UNKNOWN-NS`, `MAN-UNKNOWN-PANEL`, `MAN-UNKNOWN-LAYOUT`, `MAN-UNKNOWN-TEMPLATE` or `MAN-UNKNOWN-SETTING`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-003** If a `defaults` value fails the declared schema of its setting, or names a setting whose class is A0, then the loader shall refuse with `MAN-DEFAULT`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-004** If any command in an enabled namespace has a side-effect class listed in `forbidden_classes`, then the loader shall refuse with `MAN-FORBIDDEN-CLASS` naming the command and class.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-005** If `id` is not an `[[app]]` id in the supplied `apps.toml`, then validation shall fail with `MAN-APP-ID`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-006** The manifest shall declare `status` as `alpha`, `beta` or `stable`; if the status is not `stable`, then `status_scope` (1 to 120 characters, saying which part of the job the release covers, per D-017) shall be present, otherwise validation fails with `MAN-STATUS`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-007** Each `known_gaps` entry shall be either `{ seed = "<seed ID>" }` or `{ text = "<gap>", node = "<spec node ID>" }`, and when a seed list is supplied, an unknown seed ID shall fail with `MAN-GAP`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-008** While D-002 is undecided, the `branding` table shall require `internal = true` and placeholder values matching `^placeholder-[a-z0-9-]+$` for `name` and `icon`, otherwise validation fails with `MAN-BRANDING`.
- **REQ-ECO.APPS.MANIFEST.SCHEMA-009** The loader shall report all findings in one pass, sorted by code then name, and shall read at most 256 KiB of manifest.

## Acceptance criteria

- **AC-1** Given `fixtures/manifest/valid.toml` (id `report`, three namespaces, two panels, one layout, one template, two defaults, placeholder branding) and catalogues `fixtures/manifest/registry-manifest.json`, `panels.json`, `templates.json`, `settings.json`, `apps.toml`, when `fid-app-manifest check --manifest fixtures/manifest/valid.toml --catalogues fixtures/manifest/` runs, then it exits 0 with no findings.
- **AC-2** (error path) Given `unknown-names.toml` with one unknown namespace, one unknown panel and one unknown setting, then the check exits 1 and prints exactly three findings sorted by code; `load_manifest` in a test app returns the same three and the app does not start.
- **AC-3** (error path) Given `forbidden.toml` enabling namespace `cmm` (which holds an M1 command in the fixture registry) with `forbidden_classes = ["M1"]`, then the result is `MAN-FORBIDDEN-CLASS cmm.run_program@1 M1`.
- **AC-4** (error path) Given `extra-key.toml` containing `mcp_projection = {...}`, then the result is `MAN-SCHEMA mcp_projection`.
- **AC-5** (error path) Given `beta-no-scope.toml` (status `beta`, no `status_scope`), then `MAN-STATUS`; given `bad-id.toml` (id `suite`), then `MAN-APP-ID suite`; given `real-brand.toml` (`internal = false`), then `MAN-BRANDING`.
- **AC-6** (error path) Given `admin-default.toml` setting a default for an A0 setting, then `MAN-DEFAULT`.
- **AC-7** Given `gaps.toml` with a seed gap `PW.RPT.99` absent from `fixtures/manifest/seeds/`, when run with `--seeds fixtures/manifest/seeds/`, then `MAN-GAP PW.RPT.99`.
- **AC-8** (determinism) Running AC-2 twice gives byte-identical output.

## Design

- **Registry command(s):** none; a library plus the repository check binary `fid-app-manifest check --manifest <file> --catalogues <dir> [--seeds <dir>]`. In an app binary the loader uses the in-process registry and catalogues; in CI the check reads the same data from artifact files (registry manifest JSON from `registry.show_manifest@1`, catalogue exports).
- **Data structures:** `AppManifest { schema: 1, id, status: Alpha|Beta|Stable, status_scope: Option<String>, known_gaps: Vec<Gap>, namespaces: BTreeSet<Namespace>, forbidden_classes: BTreeSet<SideEffect>, panels: Vec<PanelId>, layout: LayoutId, templates: Vec<TemplateId>, defaults: BTreeMap<SettingKey, toml::Value>, branding: Branding { name, icon, colours?, internal } }`; `Gap::Seed(id) | Gap::Text { text, node }`; `Finding { code, name, detail }`; `Catalogues { registry, panels, layouts, templates, settings, apps, seeds? }`.
- **Algorithm:** bounded TOML parse with unknown-key rejection; collect findings for every rule; sort; refuse if any. Effective commands = registry commands whose namespace (prefix before the first dot) is enabled.
- **Errors:** finding codes above plus `MAN-IO`.
- **Determinism:** D0; pure function of the manifest and catalogues.
- **Performance budget:** under 20 ms at app start for a 5 000-command registry [unverified until measured].
- **Crate:** `fid-app-manifest` (shared app machinery in the shell layer below `app`).

## Test fixtures

Hand-written manifests, a miniature registry manifest, panel, layout, template and settings catalogues and a two-row seed file under `fixtures/manifest/` (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, create crate `fid-app-manifest` (shell layer, below `app`). Define `app.toml` schema v1 with exactly these keys: `schema = 1`, `id`, `status` (`alpha|beta|stable`), `status_scope` (required unless stable, 1–120 chars; D-017), `known_gaps` (`{seed}` or `{text, node}`), `namespaces`, `forbidden_classes` (side-effect codes), `panels`, `layout`, `templates`, `defaults` (setting key → value), `branding` (`name`, `icon`, optional `colours`, `internal`). Reject unknown keys (`MAN-SCHEMA`); there must be no key that can change a command's class or MCP projection. Implement `validate(&AppManifest, &Catalogues) -> Vec<Finding>` and `load_manifest(path, &Catalogues) -> Result<LoadedManifest, Vec<Finding>>` that report all findings at once, sorted: `MAN-UNKNOWN-NS|PANEL|LAYOUT|TEMPLATE|SETTING`, `MAN-DEFAULT` (value fails setting schema, or the setting is A0), `MAN-FORBIDDEN-CLASS` (an enabled command's class is forbidden), `MAN-APP-ID` (id not in `apps.toml`), `MAN-STATUS`, `MAN-GAP` (with optional seed list), `MAN-BRANDING` (while the project name is undecided: `internal = true` and `name`/`icon` match `^placeholder-[a-z0-9-]+$`), `MAN-IO`. Bounded read (256 KiB). Add binary `fid-app-manifest check --manifest --catalogues [--seeds]` reading catalogues from files (registry manifest JSON, panels, layouts, templates, settings, `apps.toml`). Tests for AC-1..AC-8 of spec node ECO.APPS.MANIFEST.SCHEMA. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`, fail closed), BTreeMap/BTreeSet only, no clock.

## Open questions

- The panel, layout and template catalogue export formats are owned by VIZ.SHELL.DOCK and RPT.MODEL, both open; the check reads them as JSON lists of IDs until those nodes fix a format.
- When D-002 settles the name, REQ-008 is replaced by real branding rules in a follow-up PR.

## Children

_Leaf: none._
