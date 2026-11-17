---
id: ECO.REL.WEB.ASSEMBLE
title: Bundle assembly, content-security policy and hosting guide
level: 4
parent: ECO.REL.WEB
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.WEB-001, REQ-ECO.REL.WEB-003]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [VIZ.WEB.BUILD, ECO.APPS.MANIFEST]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.WEB.ASSEMBLE: Bundle assembly, content-security policy and hosting guide

## Purpose

Assembles the web build outputs into one static directory tree and archive with relative paths only, every font, script and asset inside it, a strict content-security policy embedded in each HTML page, an example headers file for hosts, and a short hosting guide. The bundle works from any sub-path of any static server.

## Scope

**In:**
- Layout: `index.html` (app chooser), one folder per web-capable app and the viewer, `assets/` (fonts, icons), `licenses/`, `bundle-manifest.json` (file list with SHA-256)
- CSP: `default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'self'` [unverified: minimal directive set needed by the WASM runtime]
- `_headers.example` and `HOSTING.md` (MIME type for `.wasm`, cache headers, CSP header)
- `cargo xtask rel web-bundle`

**Out:**
- Building the WASM modules (owned by `VIZ.WEB.BUILD`)
- Proving no outbound requests at run time (owned by `ECO.REL.WEB.EGRESS`)
- Signing (owned by `ECO.REL.SIGN`)

## Requirements

- **REQ-ECO.REL.WEB.ASSEMBLE-001** The bundle shall contain only static files, every reference between them shall be a relative URL, and the assembler shall fail if any HTML, CSS or JS file contains an absolute `http:`, `https:` or protocol-relative URL outside comments and the licence folder.
- **REQ-ECO.REL.WEB.ASSEMBLE-002** Every HTML page shall carry the bundle CSP in a `meta` element, and the CSP's `script-src` shall not allow any origin other than `'self'`.
- **REQ-ECO.REL.WEB.ASSEMBLE-003** The bundle shall include only the apps whose manifest declares web support, plus the viewer, and `index.html` shall list exactly those.
- **REQ-ECO.REL.WEB.ASSEMBLE-004** The archive shall have sorted entries and fixed timestamps so that the same inputs give a byte-identical archive.
- **REQ-ECO.REL.WEB.ASSEMBLE-005** `bundle-manifest.json` shall list every file in the bundle with its SHA-256 and size.

## Acceptance criteria

- **AC-1** Given fixture build outputs `fixtures/rel/web/build/` (viewer plus one app), when `cargo xtask rel web-bundle --build fixtures/rel/web/build --manifests fixtures/rel/web/apps --out out/` runs, then `out/` contains the layout above and served from `python -m http.server` under the sub-path `/x/y/`, `index.html` loads the viewer (checked by the EGRESS harness).
- **AC-2** (error path) Given a fixture JS file containing `https://fonts.example.com/a.woff2`, then the assembler exits 1 with `WEB-ABSOLUTE-URL` naming file and line.
- **AC-3** Given the output, when every HTML file is parsed, then each has exactly one CSP `meta` element whose `script-src` is `'self' 'wasm-unsafe-eval'`.
- **AC-4** (determinism) Running AC-1 twice on different OSes gives byte-identical archives.

## Design

- **Registry command(s):** none; repository tool `cargo xtask rel web-bundle --build <dir> --manifests <dir> --out <dir>` (W1).
- **Data structures:** `bundle-manifest.json { version, files: [{path, sha256, size}] }`.
- **Algorithm:** copy, scan text files with a bounded tokenizer for absolute URLs, inject CSP meta, write manifest, tar and zip with sorted entries and `SOURCE_DATE_EPOCH` timestamps.
- **Errors:** `WEB-ABSOLUTE-URL(file, line)`, `WEB-MISSING-BUILD(app)`.
- **Determinism:** D0.
- **Performance budget:** under 30 s.
- **Crate:** `fid-xtask` (module `rel::web`).

## Test fixtures

`fixtures/rel/web/` with a minimal fake build (HTML, JS, a tiny WASM module) and app manifest stubs; CC-BY-4.0.

## Build prompt

> Add module `rel::web` to `fid-xtask`: `rel web-bundle --build <dir> --manifests <dir> --out <dir>`. Assemble a static bundle: `index.html` listing the viewer and every app whose manifest declares web support, one folder per app, `assets/` with bundled fonts and icons, `licenses/`, `bundle-manifest.json` (path, SHA-256, size). Inject into every HTML page a CSP meta element: default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; connect-src 'self'; object-src 'none'; base-uri 'self'; frame-ancestors 'self'. Fail with WEB-ABSOLUTE-URL on any absolute or protocol-relative URL in HTML, CSS or JS outside comments and licences. Write `_headers.example` and `HOSTING.md` (wasm MIME type, cache and CSP headers, works from any sub-path). Produce tar.gz and zip with sorted entries and fixed timestamps. Tests: AC-1..AC-4 of spec node ECO.REL.WEB.ASSEMBLE. Constraints: no panics, deny unwrap/expect.

## Open questions

- If the WASM runtime needs a worker or `blob:` URLs, the CSP gains `worker-src 'self' blob:`; settle with VIZ.WEB when the build exists [unverified].

## Children

_Leaf: none._
