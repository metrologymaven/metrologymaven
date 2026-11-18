---
id: ECO.REL.SIGN.SBOM
title: Per-artefact SPDX SBOM generation
level: 4
parent: ECO.REL.SIGN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 113
satisfies: [REQ-ECO.REL.SIGN-004]
covers: []
depends_on: []
uses_artifacts: [ECO.CI.LIC, ECO.RULES.SPDX, ECO.REL.PKG.PLAN, QUAL.SBOM]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# ECO.REL.SIGN.SBOM: Per-artefact SPDX SBOM generation

## Purpose

Generates one SPDX 2.3 JSON document per shipped artefact, listing every Rust crate compiled into it and every bundled non-Rust component (fonts, runtime libraries, WASM modules), each with name, version, source, licence expression, package URL and digest. The format was chosen jointly with `QUAL.SBOM`, which owns the content policy, the completeness check, the vulnerability scan and VEX.

## Scope

**In:**
- SPDX 2.3 JSON output, one document per artefact, named `<artefact>.spdx.json`
- Rust components from the locked dependency graph resolved for the artefact's target and features
- Non-Rust components from `release/native-components.toml` (name, version, source URL, licence, SHA-256, which artefacts bundle it)
- Deterministic document identity: namespace derived by UUIDv5 from version and artefact name; creation time from the commit time
- `cargo xtask rel sbom`

**Out:**
- Completeness check, minimum-field policy, vulnerability scan and VEX (owned by `QUAL.SBOM`)
- Licence allowlist evaluation (owned by `ECO.CI.LIC` and `ECO.RULES.SPDX`)
- Signing the SBOMs (they are covered by the pre-sign digest and the release manifest; owned by `ECO.REL.SIGN.CUSTODY`)

## Requirements

- **REQ-ECO.REL.SIGN.SBOM-001** For each artefact in the package plan, the tool shall write one SPDX 2.3 JSON document that validates against the SPDX 2.3 JSON schema [unverified schema location].
- **REQ-ECO.REL.SIGN.SBOM-002** Each document shall contain one package per Rust crate in the artefact's resolved build graph for its target and feature set, with name, version, download location, declared licence expression, a `pkg:cargo` package URL and the crate checksum from the lockfile where one exists.
- **REQ-ECO.REL.SIGN.SBOM-003** Each document shall contain one package per entry of `native-components.toml` assigned to that artefact, with its SHA-256.
- **REQ-ECO.REL.SIGN.SBOM-004** If a crate's licence expression is missing or not a valid SPDX licence expression, then the tool shall fail and name the crate.
- **REQ-ECO.REL.SIGN.SBOM-005** Given the same commit, lockfile and native-component file, the tool shall produce byte-identical documents on every OS, with the creation time set to the commit time and no wall-clock value.
- **REQ-ECO.REL.SIGN.SBOM-006** Each document shall describe the artefact itself as the root package with the artefact's SHA-256, related to every component by a `CONTAINS` relationship.

## Acceptance criteria

- **AC-1** Given fixture workspace `fixtures/rel/sbom/ws/` (three crates, one third-party dependency with MIT OR Apache-2.0, one native font entry), when `cargo xtask rel sbom --plan plan.json --workspace ws --native native-components.toml --out out/` runs, then `out/fid-cli-x86_64-linux.tar.gz.spdx.json` validates against the SPDX 2.3 schema and lists exactly the expected five packages.
- **AC-2** (error path) Given a crate with licence `GPL-2.0 OR` (invalid expression), then it exits 1 with `SBOM-LICENCE <crate>`.
- **AC-3** Given a target-specific dependency used only on Windows, then it appears in the Windows artefact's SBOM and not in the Linux one.
- **AC-4** (determinism) Running AC-1 on Windows, Linux and macOS gives byte-identical output.
- **AC-5** Given the AC-1 output, when the `QUAL.SBOM` completeness check fixture runs against it, then it passes (joint fixture).

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel sbom --plan <package-plan.json> --workspace <dir> --native <toml> --out <dir>` (W1).
- **Data structures:** SPDX 2.3 document fields used: `spdxVersion`, `dataLicense` (CC0-1.0 as SPDX requires [unverified]), `SPDXID`, `name`, `documentNamespace`, `creationInfo.created`, `creationInfo.creators` (`Tool: fid-xtask-<version>`), `packages[]`, `relationships[]`.
- **Algorithm:** for each planned artefact, resolve the dependency graph from `cargo metadata --locked --filter-platform <triple>` with the artefact's features; map each crate to a package; add native components; sort packages by (name, version); emit sorted-key JSON.
- **Build-time crates (maintainer ruling, review of PR #113):** build dependencies, proc-macro crates and every crate reached only through them are not contained in the artefact. They are listed as packages related by `<crate> BUILD_DEPENDENCY_OF SPDXRef-Artefact`; only crates reached from the artefact's binaries through normal dependency edges (not through a proc-macro crate) get `SPDXRef-Artefact CONTAINS <crate>`. Dev-only dependencies are left out.
- **Legacy licences (maintainer ruling, review of PR #113):** a Cargo `license` using the legacy `/` separator (e.g. `MIT/Apache-2.0`) is normalised to ` OR ` (Cargo's documented meaning) before validation, but only when the whole string is `term/term/...`: no whitespace, no `LicenseRef-` or `DocumentRef-`, and every part a single licence term that is not an operator; anything else (e.g. `LicenseRef-a/b`, `GPL-2.0 WITH A/B`) is left unchanged and fails; `licenseDeclared` holds the normalised expression and the package `comment` keeps the original string. Any other invalid or missing expression still fails with `SBOM-LICENCE`.
- **Package URLs of path crates:** a crate with no Cargo source (workspace member or path dependency) keeps the purl `pkg:cargo/<name>@<version>` with the qualifier `?repository_url=https://github.com/metrologymaven/metrologymaven`, so it is not mistaken for the crates.io crate of the same name; registry and git crates carry no qualifier.
- **Errors:** `SBOM-LICENCE(crate)`, `SBOM-NATIVE-DIGEST(component)`, `SBOM-PLAN-PARSE`.
- **Determinism:** D0.
- **Performance budget:** under 30 s for 20 artefacts and 600 crates [unverified].
- **Crate:** `fid-xtask` (module `rel::sbom`).

## Test fixtures

`fixtures/rel/sbom/` with a minimal workspace, a vendored copy of a tiny public-domain dummy crate, `native-components.toml` and expected outputs; CC-BY-4.0. The SPDX JSON schema is fetched by `ECO.DATA.FETCH` with its digest pinned [unverified licence of the schema file].

## Build prompt

> Add module `rel::sbom` to `fid-xtask`: `rel sbom --plan package-plan.json --workspace <dir> --native release/native-components.toml --out <dir>`. For each planned artefact, resolve its crate graph with `cargo metadata --locked --filter-platform <triple>` and its feature set, and write `<artefact>.spdx.json` in SPDX 2.3 JSON: root package = the artefact with its SHA-256; one package per crate (name, version, download location, licence expression validated as SPDX, `pkg:cargo/<name>@<version>` purl, lockfile checksum) and per native component from the TOML (with SHA-256); CONTAINS relationships from the root. documentNamespace from UUIDv5(version + artefact name); created = commit time; creators = `Tool: fid-xtask-<version>`. Fail on invalid or missing licence expressions (SBOM-LICENCE). Sort everything; byte-identical output on every OS. Tests: AC-1..AC-5 of spec node ECO.REL.SIGN.SBOM, validating against the SPDX 2.3 JSON schema. Constraints: no panics, deny unwrap/expect, no network.

## Open questions

- **Format choice (joint with QUAL.SBOM):** SPDX 2.3 JSON, because the project's licence policy already uses SPDX licence expressions and identifiers (D-001, ECO.RULES.SPDX), SPDX has an ISO/IEC 5962 edition [unverified edition mapping], and one format halves the tooling. CycloneDX was proposed in the concept doc; if QUAL.SBOM finds a scanner or VEX tool that needs it, a converter can be added without changing this node's outputs. SPDX 3.0 can be revisited when tool support is mature [unverified].
- VEX format is owned by QUAL.SBOM; OpenVEX or CSAF are candidates [unverified].
- **Resolved (review of PR #113):** build-time crates use `BUILD_DEPENDENCY_OF`, not `CONTAINS` (our reading of the SPDX 2.3 relationship types; `BUILD_TOOL_OF` was the alternative and may suit proc-macros better if QUAL.SBOM prefers). QUAL.SBOM.COMPLETE should treat both relationship types as "in the SBOM" for completeness.
- **Resolved (review of PR #113):** legacy `/` licences are normalised to `OR` with the original kept in the package `comment`, rather than refused, so real dependency graphs (e.g. `version_check` 0.9.5) do not block a release.
- **Resolved (review of PR #113):** path crates carry the purl qualifier `repository_url` (this repository) instead of omitting the purl, because QUAL.SBOM.COMPLETE requires a purl on every component.
- ECO.REL.PKG.PLAN's `PlanEntry` does not yet carry the target triple or feature set this tool reads; until it does, plan entries need `target` (and `features` where used) added.

## Children

_Leaf: none._
