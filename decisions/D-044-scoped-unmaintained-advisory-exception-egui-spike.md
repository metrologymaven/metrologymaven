---
id: D-044
title: Scoped unmaintained-advisory exception for the egui UI spike
status: accepted
raised_by: build of VIZ.SHELL.SPKTREE (PR #208), cargo-deny advisories check
blocks: [VIZ.SHELL.SPKTREE]
decide_by: before PR #208 merges
updated: 2026-11-19
---

# D-044: Scoped unmaintained-advisory exception for the egui UI spike

**Status: ACCEPTED 2026-11-19** by the maintainer: a scoped, dated, visible exception. The MSRV
is not raised and Linux AccessKit is not dropped.

## Question

`cargo deny check` fails PR #208 on four RustSec advisories of kind *unmaintained* (none is a
vulnerability), all reached only through the egui toolkit that the UI spike `fid-spike-ui`
(VIZ.SHELL.SPKTREE, toolkit chosen in D-005) pulls in as dev-dependencies of its `tree10k`
example. May `deny.toml` ignore them, and under what limits?

| Advisory | Crate | Path into the workspace |
|---|---|---|
| RUSTSEC-2026-0192 | `ttf-parser` 0.25.1 | fid-spike-ui -> egui / epaint 0.33 -> ab_glyph -> owned_ttf_parser |
| RUSTSEC-2026-0370 | `zvariant` 5.15.1 | fid-spike-ui -> egui-winit 0.33 -> accesskit_winit -> accesskit_unix (Linux) -> zbus 5 |
| RUSTSEC-2026-0371 | `zbus_names` 4.3.5 | same path as RUSTSEC-2026-0370 |
| RUSTSEC-2026-0372 | `zvariant_derive` 5.15.1 | same path as RUSTSEC-2026-0370 |

## Context

- `ttf-parser`'s author declared it unmaintained; egui 0.34 moved off it, but egui 0.34
  declares `rust-version = 1.92`, above the workspace MSRV (1.89, `Cargo.toml`).
- `zvariant`, `zbus_names` and `zvariant_derive` were folded into `zbus` 6 as modules; the
  standalone crates get no more releases. They come in through `accesskit_unix`, the AT-SPI
  bridge that exposes the spike's AccessKit tree to Linux screen readers. VIZ.SHELL.SPKTREE
  AC-4 checks announcements on Windows; the maintainer keeps the Linux bridge so the spike does
  not trade away Linux accessibility to pass a dependency check.
- `fid-spike-ui` is a throwaway spike: its library has one first-party dependency, and egui,
  winit and wgpu are dev-dependencies of one example. No kernel, model, engine or service crate,
  and no shipped surface, depends on it or on these crates.

## Options

| Option | For | Against |
|---|---|---|
| A. Scoped, dated `ignore` entries, one per advisory, guarded by a CI test | Spike unblocked; nothing else may grow the dependency; exception ends by itself | Four advisories stay in the lock until egui moves |
| B. Raise the MSRV to 1.92 and take egui 0.34 | Removes `ttf-parser` | Workspace-wide MSRV change for a throwaway spike; does not fix the zbus crates |
| C. Drop the Linux AccessKit feature | Removes the zbus crates | Loses Linux screen-reader support |
| D. Set `unmaintained = "none"` or narrow its scope | One line | Hides every future unmaintained crate in every layer |

## Decision

**A.**

1. `deny.toml` `[advisories] ignore` lists exactly RUSTSEC-2026-0192, -0370, -0371 and -0372,
   each with its own `reason` that names the crate, its path (only via fid-spike-ui -> egui /
   accesskit), states "unmaintained, not a vulnerability", and gives the expiry: **2027-03-01 or
   the egui >= 0.34 upgrade, whichever comes first**. No blanket setting, no change to the
   `unmaintained` scope, nothing else relaxed.
2. The exception is enforced fail-closed by `cargo test -p fid-xtask --test deny_scope`
   (`fid_xtask::deny_scope`), run by CI on Linux, Windows and macOS. On the `cargo metadata`
   graph for all targets and all features it fails when:
   - any workspace crate other than `fid-spike-ui` reaches one of the four crates, directly or
     transitively, through normal, build or dev dependencies (including by depending on
     `fid-spike-ui` itself);
   - a path from `fid-spike-ui` reaches `ttf-parser` without going through `epaint`, or a zbus
     crate without going through `accesskit_unix`;
   - an entry's expiry date has passed, or is later than 2027-03-01. The date is the later of
     the system date at test time and `FID_DENY_TODAY`, so an override can only move it
     forward, never keep an expired exception alive;
   - `[advisories]` holds a key other than `version`, `yanked` and `ignore` (so no
     `unmaintained` or `unsound` scope change), `yanked` is not `"deny"`, `[graph]` holds a key
     other than `all-features` (so no `exclude`, `exclude-dev` or `targets`), or
     `all-features` is not `true`;
   - egui 0.34 or later is in the graph;
   - `deny.toml` ignores an advisory that is not listed in `deny_scope::EXCEPTIONS`, or an
     entry's reason is incomplete;
   - one of the four crates has left the graph (the ignore is stale and must be removed).
3. Rationale: *unmaintained* is not a known vulnerability; the spike is not shipped in the
   kernel or engine layers (nor any other shipped layer); the path to removal is egui >= 0.34
   once the MSRV allows it; Linux screen-reader support is kept.
4. **Any future crate gets its own entry, never a widened one.** If another advisory appears,
   for example through wgpu for VIZ.RENDER.FRAME.GRAPH, it gets its own separately listed and
   justified `ignore` entry under the same rules (named crate and path, "unmaintained, not a
   vulnerability" or a vulnerability decision of its own, an expiry no later than this record
   allows, an `EXCEPTIONS` entry and a scope check), and a note on this record or a new
   decision. Existing entries are never reworded to cover another crate.

## Consequences

- PR #208's `deny` job passes with four visible, dated exceptions.
- On 2027-03-02, or when egui >= 0.34 lands, CI fails until the entries are removed (or a new
  decision renews them).
- Product crates that later adopt egui or wgpu (VIZ.RENDER, VIZ.SHELL) cannot inherit this
  exception silently: the scope test fails first.
