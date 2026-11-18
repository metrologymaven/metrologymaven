---
id: UX.SYSTEM.PKG
title: Design-system package format, versioning, changelog and deprecation
level: 3
parent: UX.SYSTEM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-design-pkg, fid-design]
satisfies: [REQ-UX.SYSTEM-002, REQ-UX.SYSTEM-003, REQ-UX.SYSTEM-004]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SYSTEM.PKG: Design-system package format, versioning, changelog and deprecation

## Purpose

The container every other UX node publishes into and every consumer (VIZ, RPT, OUTR, I18N) reads: a directory of open text files with a manifest, a semantic version, a classified changelog and a deprecation list. It also ships the reader library and two checks: a structural package check and a version-diff check that refuses a release whose version bump does not match its changes.

## Scope

**In:**
- Package layout and the manifest `design/package.toml` (`fid-design-package@1`)
- Token file format: the W3C Design Tokens Community Group JSON format, pinned to one named draft, with project constraints (see Design)
- Name index: every token, icon, GD&T symbol, component, pattern, palette and screen name with its kind
- `CHANGELOG.md` entry format with breaking, additive and fix classes; `deprecations.json`
- Reader library `fid-design-pkg` (read-only; consumers link it or read the files directly)
- `fid-design pkg-check` and `fid-design pkg-diff`

**Out:** the content of tokens, icons, specs and screens (the sibling UX nodes); the literal-value lint (`UX.SYSTEM.LINT`); runtime token loading (`VIZ.SHELL.THEME`); release publishing of the package (ECO.REL).

## Requirements

- **REQ-UX.SYSTEM.PKG-001** The package shall consist only of UTF-8 text files (TOML, JSON, Markdown, SVG) plus PNG reference renderings, and shall be readable and diffable without any design tool.
- **REQ-UX.SYSTEM.PKG-002** The manifest shall hold the format ID, the package semantic version, the token-format draft ID and the placeholder product name `{{FAMILY_NAME}}`.
- **REQ-UX.SYSTEM.PKG-003** Every published name shall be unique across kinds within the name index and shall match `^[a-z][a-z0-9]*(\.[a-z0-9-]+)*$`.
- **REQ-UX.SYSTEM.PKG-004** When `fid-design pkg-diff --from <old> --to <new>` runs, it shall classify each name as added, removed, renamed (by deprecation entry), value-changed or unchanged, and shall fail if any removal or rename is not accompanied by a major version increment.
- **REQ-UX.SYSTEM.PKG-005** If a changelog entry is missing for a changed name, or an entry's class contradicts the diff classification, then `pkg-diff` shall fail and name it.
- **REQ-UX.SYSTEM.PKG-006** If a name is removed in a release less than one minor version after the release that first listed it in `deprecations.json`, or a deprecation entry lacks a replacement, then `pkg-diff` shall fail.
- **REQ-UX.SYSTEM.PKG-007** While a name is deprecated, the reader library shall resolve it to its replacement's value and report a deprecation warning naming both.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/system/pkg/v1.0.0/`, when `fid-design pkg-check --root v1.0.0 --json` runs, then it exits 0 and reports the name counts per kind.
- **AC-2** Given `v1.0.0/` and `v1.1.0/` (one token added, one deprecated with a replacement, changelog classifies both), when `pkg-diff` runs, then it exits 0.
- **AC-3** (error path) Given `v1.0.0/` and `v1.1.0-removed/` (a token removed in a minor release), when `pkg-diff` runs, then it exits 1 with `PKG-BREAKING-MINOR <name>`.
- **AC-4** (error path) Given `v1.2.0-nolog/` (a value changed, no changelog entry), when `pkg-diff` runs, then it exits 1 with `PKG-CHANGELOG-MISSING <name>`.
- **AC-5** Given `v1.1.0/`, when the reader resolves the deprecated token, then it returns the replacement's value and one `deprecated_name` warning.
- **AC-6** (error path) Given `dup-name/` (an icon and a token both named `status.ok`), when `pkg-check` runs, then it exits 1 with `PKG-NAME-DUPLICATE status.ok`.

## Design

- **Commands:** `fid-design pkg-check --root <dir> [--json]`; `fid-design pkg-diff --from <dir> --to <dir> [--json]`. Tooling, nearest class R0.
- **Layout:** `package.toml`, `CHANGELOG.md`, `deprecations.json`, `names.json` (generated index), `principles.md`, `writing-guide.md`, `reserved-words.json`, `tokens/`, `icons/`, `gdt/`, `components/`, `patterns/`, `screens/`, `report/`, `review/`, `research/`, `sources.json`.
- **Token format:** DTCG JSON pinned by draft ID in the manifest [unverified stability of the DTCG drafts]; constraints: aliases only by `{name}` reference, no expressions, colours in sRGB hex `#rrggbb` or `#rrggbbaa`, dimensions in `px` (logical pixels) or `pt` (print), durations in `ms`. If the pinned draft changes incompatibly, the reader keeps parsing the pinned draft until a major package release migrates.
- **Changelog entry:** `- <class>: <name> - <one line>` under a `## <version>` heading; class in `breaking|additive|fix`.
- **Deprecation entry:** `{ name, replacement, since: "<version>", remove_after: "<version>" }`.
- **Errors:** `PKG-SCHEMA`, `PKG-NAME-DUPLICATE`, `PKG-NAME-SYNTAX`, `PKG-ALIAS-UNRESOLVED`, `PKG-BREAKING-MINOR`, `PKG-CHANGELOG-MISSING`, `PKG-CHANGELOG-CLASS`, `PKG-DEPRECATION-EARLY`, `PKG-DEPRECATION-NOREPL`.
- **Determinism:** D0 (BTreeMap ordering; no timestamps).
- **Crate:** `fid-design-pkg` (reader library, Tooling layer, no I/O beyond reading the given root) and `fid-design` (commands).

## Test fixtures

`fixtures/ux/system/pkg/`: `v1.0.0/`, `v1.1.0/`, `v1.1.0-removed/`, `v1.2.0-nolog/`, `dup-name/`; synthetic, CC-BY-4.0.

## Build prompt

> Create the Rust library crate `fid-design-pkg` (read-only reader of a design-system package directory) and the commands `fid-design pkg-check --root <dir> [--json]` and `fid-design pkg-diff --from <dir> --to <dir> [--json]` in the tooling crate `fid-design`.
>
> Package: `package.toml` (`format = "fid-design-package@1"`, `version` semver, `token_format` draft ID, `product_name = "{{FAMILY_NAME}}"`), `CHANGELOG.md` (`## <version>` then `- breaking|additive|fix: <name> - text`), `deprecations.json` (`[{name, replacement, since, remove_after}]`), token files in DTCG JSON (aliases `{name}` only; colours `#rrggbb[aa]`; dimensions px or pt; durations ms), and a generated `names.json` index of every token, icon, GD&T symbol, component, pattern, palette and screen name with its kind. Names match `^[a-z][a-z0-9]*(\.[a-z0-9-]+)*$` and are unique across kinds. The reader resolves aliases (cycle-safe), resolves deprecated names to their replacement with a warning.
>
> `pkg-diff` classifies names as added, removed, renamed, value-changed, unchanged; fails with `PKG-BREAKING-MINOR` when a removal or rename lacks a major bump, `PKG-CHANGELOG-MISSING`/`PKG-CHANGELOG-CLASS` when the changelog disagrees, `PKG-DEPRECATION-EARLY`/`PKG-DEPRECATION-NOREPL` for deprecation violations. Exit 0, 1 findings, 2 unreadable. Tests AC-1..AC-6 of spec node UX.SYSTEM.PKG with fixtures under `fixtures/ux/system/pkg/`.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing (file at most 8 MiB, JSON depth 32, alias depth 16); BTreeMap only; no network; allowlisted dependencies (CONSTITUTION §8).

## Open questions

- The DTCG format's stability is [unverified]; pinning one draft ID in the manifest isolates consumers from upstream changes.
- Publishing the package as a release artifact is ECO.REL's job; consumers then declare `uses_artifacts` on the UX node that owns the part they read.

## Children

_Leaf: none._
