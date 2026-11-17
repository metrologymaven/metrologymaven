---
id: ECO.REL.PKG.LINUX
title: Linux packages
level: 4
parent: ECO.REL.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.REL.PKG-001, REQ-ECO.REL.PKG-002, REQ-ECO.REL.PKG-003, REQ-ECO.REL.PKG-006]
covers: []
depends_on: [ECO.CI.WF]
uses_artifacts: [ECO.REL.PKG.PLAN, ECO.REL.PKG.REG, PLAT.DEPLOY.NOADMIN, PLAT.DEPLOY.OFFLINE]
side_effect: W1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REL.PKG.LINUX: Linux packages

## Purpose

Builds the Linux packages listed in the package plan: a portable `.tar.gz` per package that a user installs into their home directory without root (the default, per-user path), plus `.deb` and `.rpm` packages for administrators who install system-wide. The archive carries a small offline install script that places the files and writes the XDG desktop entry and registration entry `ECO.REL.PKG.REG` defines.

## Scope

**In:**
- `.tar.gz` per app, `fid` and `fid-verify`, with `install.sh` and `uninstall.sh` (POSIX sh, no network)
- `.deb` and `.rpm` per app, `fid` and `fid-verify`, installing under `/opt/<product>/` with links in `/usr/bin`
- XDG desktop entries and MIME type for the project container
- Smoke tests in containers of the Tier 1 distributions with no network

**Out:**
- RPM signing (owned by `ECO.REL.SIGN.OSSIGN`)
- Flatpak, Snap and AppImage (not planned; maintainer decision)
- Package repositories (apt or yum repositories need hosting; maintainer decision)

## Requirements

- **REQ-ECO.REL.PKG.LINUX-001** For every Linux entry in the package plan, the build shall produce the `.tar.gz`, `.deb` and `.rpm` files named in the plan.
- **REQ-ECO.REL.PKG.LINUX-002** When `install.sh` is run by a non-root user, it shall install into `$XDG_DATA_HOME` (default `~/.local/share`) and `~/.local/bin`, write the desktop entry and registration entry, and require no root rights.
- **REQ-ECO.REL.PKG.LINUX-003** `install.sh` and `uninstall.sh` shall use only POSIX sh and core utilities and shall make no network request.
- **REQ-ECO.REL.PKG.LINUX-004** The `.deb` and `.rpm` packages shall declare no maintainer script that opens a network connection, and their dependency fields shall list only packages present on the Tier 1 distributions' base images.
- **REQ-ECO.REL.PKG.LINUX-005** The `fid-verify` packages shall contain only the `fid-verify` binary, licence files and documentation.
- **REQ-ECO.REL.PKG.LINUX-006** `uninstall.sh` shall remove every file `install.sh` wrote and leave user projects and settings untouched.

## Acceptance criteria

- **AC-1** Given fixture plan `fixtures/rel/pkglinux/plan.json` and stub binaries, when the Linux build job runs, then every planned file exists with the planned name.
- **AC-2** Given each Tier 1 distribution container started with `--network none` and a non-root user, when `install.sh` from the `report` archive runs, then it exits 0, `report --self-check` exits 0, and the desktop and registration entries exist.
- **AC-3** Given the same container as root, when the `.deb` (or `.rpm` on RPM distributions) installs with the OS package tool offline, then it succeeds and `report --self-check` exits 0.
- **AC-4** Given AC-2, when `uninstall.sh` runs, then no file written by `install.sh` remains and `~/projects/fixture` is unchanged.
- **AC-5** Given `shellcheck` on `install.sh` and `uninstall.sh`, then no warning of severity warning or higher is reported, and a grep for network tools (`curl`, `wget`, `nc`) finds none.

## Design

- **Registry command(s):** none; CI job plus `cargo xtask rel pkg-linux --plan <file> --bin <dir> --out <dir>` (W1).
- **Data structures:** templates under `packaging/linux/` (desktop entry, MIME XML, control and spec templates, install scripts).
- **Algorithm:** archives with sorted entries, fixed mtimes (commit time) and numeric owner 0; `.deb` and `.rpm` built with standard tools [unverified reproducibility of rpm headers].
- **Errors:** per-package build failures reported to the plan result directory.
- **Determinism:** D1.
- **Performance budget:** under 10 min [unverified].
- **Crate:** `fid-xtask` (module `rel::pkg_linux`); templates under `packaging/linux/`.

## Test fixtures

`fixtures/rel/pkglinux/` with a plan and stub binaries; container images of the Tier 1 distributions pulled by digest; CC-BY-4.0 for project fixtures.

## Build prompt

> Add `cargo xtask rel pkg-linux --plan package-plan.json --bin <dir> --out <dir>`. For each Linux plan entry build: a `.tar.gz` with sorted entries, fixed mtimes and owner 0, containing the binaries plus POSIX-sh `install.sh` (installs per user into $XDG_DATA_HOME and ~/.local/bin, writes the XDG desktop entry, MIME type and the ECO.REL.PKG.REG registration entry, no root, no network) and `uninstall.sh` (removes exactly what install wrote); a `.deb` and an `.rpm` installing under /opt/<product>/ with /usr/bin links and no networked maintainer scripts. The fid-verify packages contain only the binary, licences and docs. Add CI smoke tests in containers of each Tier 1 distribution with `--network none`: per-user install, self-check, uninstall, and offline root install of deb or rpm. Run shellcheck on the scripts. Tests: AC-1..AC-5 of spec node ECO.REL.PKG.LINUX. Do not sign (ECO.REL.SIGN.OSSIGN signs rpm after approval).

## Open questions

- Distribution-specific dependency names (for example GPU and windowing libraries) depend on the final UI toolkit (D-005 provisional) [unverified].

## Children

_Leaf: none._
