#!/usr/bin/env python3
"""Compute fail-closed CI scope for pull requests, pushes and merge queues.

Stdlib only. Maps a git diff plus `cargo metadata` reverse dependencies into
fine-grained workflow outputs so:
- spec/docs-only PRs skip Rust, Windows, macOS, wasm32, kernel-repro and xart;
- narrow crate PRs compile and test only the affected reverse-dependency closure;
- full workspace, cross-OS, stress, kernel-repro and xart gates always run on
  `main`, `merge_group`, `workflow_dispatch`, tags, or workspace-global edits.

Usage:
    python3 tools/ci_scope.py --event pull_request --base <sha> --head <sha>
                              [--github-output "$GITHUB_OUTPUT"]
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tomllib
from collections import defaultdict, deque
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ZERO_SHA = "0" * 40

# Paths that NEVER affect Rust compilation, tests, fixtures, VERIFY.md, docs/dmis,
# docs/platform/support-matrix.md, or docs/contributor/guide.md.
NON_RUST_PREFIXES = (
    "spec/",
    "decisions/",
    "seeds/",
    "milestones/",
    "research/",
    "generated/",
    "site/",
    "community/",
)
NON_RUST_FILES = frozenset(
    {
        "README.md",
        "AGENTS.md",
        "CLAUDE.md",
        "CONTRIBUTING.md",
        "GOVERNANCE.md",
        "PROMOTE.md",
        "ATTRIBUTION.md",
        "LICENSE.md",
        ".gitignore",
    }
)

# Crates in the kernel / xart security-and-determinism boundary.
KERNEL_ROOT_CRATES = ("fid-kernel-wasm", "fid-wasm-check")
XART_ROOT_CRATES = ("fid-kernel-wasm", "fid-wasm-check", "fid-xart")

# Paths that affect the entire workspace and force `mode=full` on PRs.
FULL_MODE_FILES = frozenset(
    {
        "Cargo.toml",
        "rustfmt.toml",
        "deny.toml",
        "kernel-allowlist.toml",
        "apps-policy.toml",
        "storage-writers.toml",
        "datasets.toml",
        "bots.toml",
        "lint-allowlist.toml",
        ".github/workflows/rust.yml",
    }
)
FULL_MODE_PREFIXES = (
    ".cargo/",
    "vendor/",
    "tools/",
    "scripts/",
    "kernel-build/",
    "platform/",
    "design/",
    "schema/",
    "schemas/",
    "data/",
    "house-rules/",
    "release/",
)

# If affected crates exceed this fraction of the workspace, just run `--workspace`.
AFFECTED_FULL_THRESHOLD = 0.55


def parse_lockfile_external(text: str) -> dict[tuple[str, str], str]:
    """Return {(name, version): checksum_or_source} for external (non-workspace) packages."""
    data = tomllib.loads(text)
    out: dict[tuple[str, str], str] = {}
    for pkg in data.get("package", []):
        source = pkg.get("source")
        if not source:
            continue
        name = str(pkg.get("name", ""))
        version = str(pkg.get("version", ""))
        checksum = str(pkg.get("checksum", source))
        out[(name, version)] = checksum
    return out


def external_deps_diff(base_lock: str | None, head_lock: str | None) -> set[str]:
    """Return external crate names whose version/checksum/presence changed between lockfiles."""
    if base_lock is None or head_lock is None:
        return {"*"}
    try:
        b = parse_lockfile_external(base_lock)
        h = parse_lockfile_external(head_lock)
    except Exception:
        return {"*"}
    if b == h:
        return set()
    changed: set[str] = set()
    all_keys = set(b) | set(h)
    for k in all_keys:
        if b.get(k) != h.get(k):
            changed.add(k[0])
    return changed


def load_wasm32_crates(root: Path = ROOT) -> set[str]:
    path = root / "ci" / "wasm32-crates.txt"
    out: set[str] = set()
    if not path.is_file():
        return out
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        out.add(line.split()[0])
    return out


def load_lockfile_dep_graph(root: Path = ROOT) -> tuple[dict[str, set[str]], set[str]]:
    """Parse Cargo.lock into ({pkg_name: set(dep_pkg_names)}, set(external_pkg_names))."""
    lock_path = root / "Cargo.lock"
    if not lock_path.is_file():
        return {}, set()
    data = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    deps: dict[str, set[str]] = defaultdict(set)
    external: set[str] = set()
    for pkg in data.get("package", []):
        name = str(pkg.get("name", ""))
        if not name:
            continue
        if pkg.get("source"):
            external.add(name)
        for dep in pkg.get("dependencies", []):
            # Entries look like "serde" or "serde 1.0.0" or "serde 1.0.0 (registry+...)".
            dname = str(dep).split()[0]
            if dname:
                deps[name].add(dname)
    return deps, external


def load_workspace_graph(
    root: Path = ROOT,
) -> tuple[
    dict[str, Path],
    dict[str, set[str]],
    dict[str, set[str]],
    dict[str, set[str]],
]:
    """Load Cargo workspace graph completely offline via `--no-deps` + `Cargo.lock`.

    Returns:
      - crate_dirs: {crate_name: Path relative to root}
      - rev_deps:   {crate_name: set of workspace crates that depend on it (normal/dev/build)}
      - fwd_deps:   {crate_name: set of workspace crates it depends on}
      - ext_deps:   {crate_name: set of transitive external crate names}
    """
    proc = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    meta = json.loads(proc.stdout)
    lock_deps, lock_ext = load_lockfile_dep_graph(root)
    return parse_cargo_metadata(meta, root, lock_deps, lock_ext)


def parse_cargo_metadata(
    meta: dict,
    root: Path,
    lock_deps: dict[str, set[str]] | None = None,
    lock_ext: set[str] | None = None,
) -> tuple[
    dict[str, Path],
    dict[str, set[str]],
    dict[str, set[str]],
    dict[str, set[str]],
]:
    ws_ids = set(meta.get("workspace_members", []))
    by_id: dict[str, dict] = {p["id"]: p for p in meta.get("packages", [])}
    ws_names: set[str] = set()
    crate_dirs: dict[str, Path] = {}
    for pid in ws_ids:
        p = by_id[pid]
        name = p["name"]
        ws_names.add(name)
        manifest = Path(p["manifest_path"])
        try:
            rel = manifest.parent.resolve().relative_to(root.resolve())
        except ValueError:
            rel = manifest.parent
        crate_dirs[name] = rel

    rev_deps: dict[str, set[str]] = defaultdict(set)
    fwd_deps: dict[str, set[str]] = defaultdict(set)
    ext_deps: dict[str, set[str]] = defaultdict(set)

    # Direct edges from Cargo.toml declarations (normal, dev, and build dependencies).
    for pid in ws_ids:
        p = by_id[pid]
        src = p["name"]
        for dep in p.get("dependencies", []):
            dname = dep["name"]
            if dname in ws_names and dname != src:
                fwd_deps[src].add(dname)
                rev_deps[dname].add(src)
            elif dname not in ws_names:
                ext_deps[src].add(dname)

    # Transitive external dependency names from Cargo.lock (offline, instant).
    if lock_deps and lock_ext:
        for src in ws_names:
            seen: set[str] = set()
            q = deque(lock_deps.get(src, ()))
            while q:
                cur = q.popleft()
                if cur in seen:
                    continue
                seen.add(cur)
                if cur in lock_ext:
                    ext_deps[src].add(cur)
                for nxt in lock_deps.get(cur, ()):
                    if nxt not in seen:
                        q.append(nxt)

    return crate_dirs, rev_deps, fwd_deps, ext_deps


def closure(seeds: set[str], edges: dict[str, set[str]]) -> set[str]:
    out = set(seeds)
    q = deque(seeds)
    while q:
        cur = q.popleft()
        for nxt in edges.get(cur, ()):
            if nxt not in out:
                out.add(nxt)
                q.append(nxt)
    return out


def is_non_rust_path(path: str) -> bool:
    if path in NON_RUST_FILES:
        return True
    return any(path.startswith(p) for p in NON_RUST_PREFIXES)


def map_fixture_to_crates(fixture_path: str, crate_dirs: dict[str, Path], root: Path) -> set[str]:
    """Find workspace crates whose src/ or tests/ mention this fixture's directory."""
    parts = Path(fixture_path).parts
    # Look for references to `fixtures/<a...>` using the directory of the fixture.
    if len(parts) < 2 or parts[0] != "fixtures":
        return set()
    # Use up to 3 leading path components (`fixtures/<a>/<b>` or `fixtures/<a>`).
    prefixes = []
    if len(parts) >= 3:
        prefixes.append("/".join(parts[:3]))
    prefixes.append("/".join(parts[:2]))
    matched: set[str] = set()
    for cname, cdir in crate_dirs.items():
        abs_dir = root / cdir
        for sub in ("src", "tests"):
            sdir = abs_dir / sub
            if not sdir.is_dir():
                continue
            for rs in sdir.rglob("*.rs"):
                try:
                    txt = rs.read_text(encoding="utf-8", errors="ignore")
                except OSError:
                    continue
                if any(p in txt for p in prefixes):
                    matched.add(cname)
                    break
            if cname in matched:
                break
    return matched


def compute_scope(
    *,
    event: str,
    files: list[str] | None,
    crate_dirs: dict[str, Path],
    rev_deps: dict[str, set[str]],
    fwd_deps: dict[str, set[str]],
    ext_deps: dict[str, set[str]],
    wasm32_crates: set[str],
    changed_external_deps: set[str],
    root: Path = ROOT,
) -> dict[str, str]:
    """Compute all workflow scope outputs. Fail-closed to full on unknown inputs."""
    all_crates = set(crate_dirs)

    def full_scope() -> dict[str, str]:
        return {
            "rust": "true",
            "mode": "full",
            "crates": "",
            "affected_count": str(len(all_crates)),
            "wasm_crates": "ALL",
            "run_wasm_kernel": "true",
            "run_wasm32": "true",
            "run_msrv": "true",
            "run_deny": "true",
            "run_verify_guide": "true",
            "run_tmpl_fuzz": "true",
            "run_ooc_full": "true",
            "run_batch_full": "true",
            "run_nurbs_oracle": "true",
            "run_dmis_conf": "true",
            "run_site_import": "true",
            "run_contrib_guide": "true",
            "run_repo_tpl": "true",
            "run_fuzz_checks": "true",
            "run_kernel_repro": "true",
            "run_xart": "true",
        }

    # Always run full CI on pushes to main, merge_group, tags, manual dispatch, or unknown diff.
    if event != "pull_request" or files is None:
        return full_scope()

    if not files or all(is_non_rust_path(f) for f in files):
        return {
            "rust": "false",
            "mode": "skip",
            "crates": "",
            "affected_count": "0",
            "wasm_crates": "NONE",
            "run_wasm_kernel": "false",
            "run_wasm32": "false",
            "run_msrv": "false",
            "run_deny": "false",
            "run_verify_guide": "false",
            "run_tmpl_fuzz": "false",
            "run_ooc_full": "false",
            "run_batch_full": "false",
            "run_nurbs_oracle": "false",
            "run_dmis_conf": "false",
            "run_site_import": "false",
            "run_contrib_guide": "false",
            "run_repo_tpl": "false",
            "run_fuzz_checks": "false",
            "run_kernel_repro": "false",
            "run_xart": "false",
        }

    # Build directory -> crate lookup (longest path first).
    dir_to_crate = sorted(
        ((cdir.as_posix().rstrip("/") + "/", cname) for cname, cdir in crate_dirs.items()),
        key=lambda item: len(item[0]),
        reverse=True,
    )

    touched_crates: set[str] = set()
    force_full = bool(changed_external_deps)

    for f in files:
        if is_non_rust_path(f):
            continue
        if f in FULL_MODE_FILES or any(f.startswith(p) for p in FULL_MODE_PREFIXES):
            force_full = True
            continue
        if f == "Cargo.lock":
            # External dep changes already captured in `changed_external_deps`.
            continue
        if f in ("ci/fixture-checks.txt", "ci/wasm32-crates.txt"):
            continue
        if f == "layers.toml":
            if "fid-xtask" in all_crates:
                touched_crates.add("fid-xtask")
            continue
        matched_crate = None
        for prefix, cname in dir_to_crate:
            if f.startswith(prefix):
                matched_crate = cname
                break
        if matched_crate is not None:
            touched_crates.add(matched_crate)
            continue
        if f.startswith("fixtures/"):
            mapped = map_fixture_to_crates(f, crate_dirs, root)
            if mapped:
                touched_crates |= mapped
            else:
                force_full = True
            continue
        if f in ("VERIFY.md", "KEYS.md") or f.startswith("keys/"):
            if "fid-xtask" in all_crates:
                touched_crates.add("fid-xtask")
            continue
        if f.startswith("docs/"):
            # Docs checked by specific crates.
            if f.startswith("docs/dmis/") and "fid-io-dmis" in all_crates:
                touched_crates.add("fid-io-dmis")
            elif f.startswith("docs/platform/") and "fid-platform" in all_crates:
                touched_crates.add("fid-platform")
            elif f.startswith("docs/contributor/"):
                for c in ("fid-brand-lint", "fid-outreach"):
                    if c in all_crates:
                        touched_crates.add(c)
            else:
                force_full = True
            continue
        # Any unknown path fails closed to full mode.
        force_full = True

    if not touched_crates:
        force_full = True

    affected = all_crates if force_full else closure(touched_crates, rev_deps)
    if not force_full and len(all_crates) > 0 and (len(affected) / len(all_crates)) > AFFECTED_FULL_THRESHOLD:
        force_full = True
        affected = all_crates

    mode = "full" if force_full else "affected"
    crates_flags = "" if force_full else " ".join(f"-p {c}" for c in sorted(affected))

    # Kernel and xart forward closures (packages that build into the WASM kernel / xart).
    kernel_closure = closure({c for c in KERNEL_ROOT_CRATES if c in all_crates}, fwd_deps)
    xart_closure = closure({c for c in XART_ROOT_CRATES if c in all_crates}, fwd_deps)
    kernel_ext_deps = set().union(*(ext_deps.get(c, set()) for c in kernel_closure)) if kernel_closure else set()
    xart_ext_deps = set().union(*(ext_deps.get(c, set()) for c in xart_closure)) if xart_closure else set()

    kernel_ext_changed = "*" in changed_external_deps or bool(changed_external_deps & kernel_ext_deps)
    xart_ext_changed = "*" in changed_external_deps or bool(changed_external_deps & xart_ext_deps)

    def any_path(*prefixes: str) -> bool:
        return any(f == p or f.startswith(p) for f in files for p in prefixes)

    run_kernel_repro = (
        any_path(
            ".github/workflows/kernel-repro.yml",
            "kernel-build/",
            "scripts/rebuild-kernel.sh",
            "fixtures/man/repro/",
            "Cargo.toml",
            ".cargo/",
        )
        or bool(touched_crates & kernel_closure)
        or kernel_ext_changed
    )

    run_xart = (
        any_path(
            ".github/workflows/xart.yml",
            "fixtures/man/xart-calls-v1/",
            "Cargo.toml",
            ".cargo/",
        )
        or bool(touched_crates & xart_closure)
        or xart_ext_changed
    )

    run_wasm_kernel = (
        force_full
        or run_kernel_repro
        or run_xart
        or any_path("scripts/manifest-gen-ci.sh", "fixtures/man/")
        or "fid-manifest-gen" in affected
    )

    if force_full or any_path("ci/wasm32-crates.txt"):
        wasm_crates_out = "ALL"
    else:
        wasm_hit = sorted(affected & wasm32_crates)
        wasm_crates_out = ",".join(wasm_hit) if wasm_hit else "NONE"

    run_wasm32 = wasm_crates_out != "NONE" or run_wasm_kernel
    run_deny = (
        bool(changed_external_deps)
        or any_path("deny.toml", "Cargo.toml", ".github/workflows/rust.yml")
        or any(f.endswith("/Cargo.toml") for f in files)
    )
    run_verify_guide = any_path(
        "VERIFY.md",
        "KEYS.md",
        "keys/",
        "tools/verify_guide.py",
        "fixtures/rel/guide/",
        ".github/workflows/rust.yml",
    )
    run_tmpl_fuzz = force_full or "fid-report" in affected or any_path("fixtures/rpt/")
    run_ooc_full = force_full or "fid-geo-ooc" in affected or any_path("fixtures/geo/ooc/")
    run_batch_full = force_full or "fid-geo-dist" in affected or any_path("fixtures/geo/dist/")
    run_nurbs_oracle = (
        force_full
        or "fid-geo-nurbs" in affected
        or any_path("fixtures/geo/nurbs/", "tools/fixtures/nurbs_oracle.py")
    )
    run_dmis_conf = force_full or "fid-io-dmis" in affected or any_path("docs/dmis/")
    run_site_import = force_full or "fid-outreach" in affected or any_path("fixtures/outr/site/import/")
    run_contrib_guide = (
        force_full
        or bool(affected & {"fid-brand-lint", "fid-outreach"})
        or any_path("docs/contributor/guide.md")
    )
    run_repo_tpl = force_full or any_path("tools/repo_template/", "fixtures/repo/tpl/")
    run_fuzz_checks = force_full or bool(affected & {"fid-program", "fid-xml", "fid-cad-proto", "fid-io-las"})

    b = lambda v: "true" if v else "false"
    return {
        "rust": "true",
        "mode": mode,
        "crates": crates_flags,
        "affected_count": str(len(affected)),
        "wasm_crates": wasm_crates_out,
        "run_wasm_kernel": b(run_wasm_kernel),
        "run_wasm32": b(run_wasm32),
        "run_msrv": "true",
        "run_deny": b(run_deny),
        "run_verify_guide": b(run_verify_guide),
        "run_tmpl_fuzz": b(run_tmpl_fuzz),
        "run_ooc_full": b(run_ooc_full),
        "run_batch_full": b(run_batch_full),
        "run_nurbs_oracle": b(run_nurbs_oracle),
        "run_dmis_conf": b(run_dmis_conf),
        "run_site_import": b(run_site_import),
        "run_contrib_guide": b(run_contrib_guide),
        "run_repo_tpl": b(run_repo_tpl),
        "run_fuzz_checks": b(run_fuzz_checks),
        "run_kernel_repro": b(run_kernel_repro),
        "run_xart": b(run_xart),
    }


def git_changed_files(base: str, head: str, root: Path = ROOT) -> list[str] | None:
    if not base or base == ZERO_SHA:
        return None
    chk = subprocess.run(
        ["git", "cat-file", "-e", f"{base}^{{commit}}"],
        cwd=root,
        capture_output=True,
    )
    if chk.returncode != 0:
        return None
    proc = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...{head}"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if proc.returncode != 0:
        # Fallback to two-dot diff when merge-base is unavailable (shallow clone).
        proc = subprocess.run(
            ["git", "diff", "--name-only", base, head],
            cwd=root,
            capture_output=True,
            text=True,
        )
        if proc.returncode != 0:
            return None
    return [line.strip() for line in proc.stdout.splitlines() if line.strip()]


def git_show(rev: str, path: str, root: Path = ROOT) -> str | None:
    proc = subprocess.run(
        ["git", "show", f"{rev}:{path}"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    return proc.stdout if proc.returncode == 0 else None


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Compute fail-closed CI scope for GitHub Actions.")
    ap.add_argument("--event", default=os.environ.get("GITHUB_EVENT_NAME", "pull_request"))
    ap.add_argument("--base", default=os.environ.get("CI_BASE_SHA", ""))
    ap.add_argument("--head", default=os.environ.get("GITHUB_SHA", "HEAD"))
    ap.add_argument("--github-output", default=os.environ.get("GITHUB_OUTPUT", ""))
    ap.add_argument("--files", nargs="*", default=None, help="Explicit changed files (for testing)")
    args = ap.parse_args(argv)

    crate_dirs, rev_deps, fwd_deps, ext_deps = load_workspace_graph(ROOT)
    wasm32_crates = load_wasm32_crates(ROOT)

    if args.files is not None:
        files: list[str] | None = list(args.files)
        changed_ext: set[str] = set()
    elif args.event == "pull_request":
        files = git_changed_files(args.base, args.head, ROOT)
        if files is not None and "Cargo.lock" in files:
            base_lock = git_show(args.base, "Cargo.lock", ROOT)
            head_lock = git_show(args.head, "Cargo.lock", ROOT)
            changed_ext = external_deps_diff(base_lock, head_lock)
        else:
            changed_ext = set()
    else:
        files = None
        changed_ext = set()

    outputs = compute_scope(
        event=args.event,
        files=files,
        crate_dirs=crate_dirs,
        rev_deps=rev_deps,
        fwd_deps=fwd_deps,
        ext_deps=ext_deps,
        wasm32_crates=wasm32_crates,
        changed_external_deps=changed_ext,
        root=ROOT,
    )

    for k, v in outputs.items():
        print(f"{k}={v}")

    if args.github_output:
        with open(args.github_output, "a", encoding="utf-8") as fh:
            for k, v in outputs.items():
                fh.write(f"{k}={v}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
