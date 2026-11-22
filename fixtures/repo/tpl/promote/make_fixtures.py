#!/usr/bin/env python3
"""Independent fixture generator for spec node ECO.REPO.TPL.PROMOTE.

Generates:
  - fixtures/repo/tpl/promote/mono.bundle: synthetic monorepo with crates fid-a and fid-b,
    12 commits total, 5 touching crates/fid-b, with fid-a depending on fid-b by path.
  - fixtures/repo/tpl/promote/repos.toml: fixture repository map listing fid-b at
    https://example.invalid/fid-b (for AC-2 `cargo xtask pins`).

Run with `--check` to verify the committed fixtures match this generator.
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
BUNDLE = HERE / "mono.bundle"
REPOS_TOML = HERE / "repos.toml"

REPOS_TOML_TEXT = """# Fixture repository map for ECO.REPO.TPL.PROMOTE AC-2.

[[repo]]
id = "fid-b"
url = "https://example.invalid/fid-b"
purpose = "Promoted fixture library fid-b"
protected_tags = true
"""

COMMITS = [
    # 1 (root + fid-a only)
    (
        "Alice Dev",
        "alice@example.invalid",
        "2026-01-01T10:00:00Z",
        "chore: initialize workspace and fid-a",
        {
            "Cargo.toml": '[workspace]\nmembers = ["crates/fid-a", "crates/fid-b"]\nresolver = "2"\n',
            "crates/fid-a/Cargo.toml": (
                '[package]\nname = "fid-a"\nversion = "0.1.0"\nedition = "2021"\n\n'
                '[dependencies]\n'
                'fid-b = { path = "../fid-b" }\n'
            ),
            "crates/fid-a/src/lib.rs": "pub fn a() -> u32 { fid_b::b() + 1 }\n",
        },
    ),
    # 2 (touches fid-b #1)
    (
        "Bob Lib",
        "bob@example.invalid",
        "2026-01-02T10:00:00Z",
        "feat(fid-b): initial crate scaffold",
        {
            "crates/fid-b/Cargo.toml": '[package]\nname = "fid-b"\nversion = "0.1.0"\nedition = "2021"\n',
            "crates/fid-b/src/lib.rs": "pub fn b() -> u32 { 1 }\n",
        },
    ),
    # 3 (fid-a only)
    (
        "Alice Dev",
        "alice@example.invalid",
        "2026-01-03T10:00:00Z",
        "feat(fid-a): add helper",
        {
            "crates/fid-a/src/lib.rs": "pub fn a() -> u32 { fid_b::b() + 1 }\npub fn helper() -> u32 { 10 }\n",
        },
    ),
    # 4 (touches fid-b #2)
    (
        "Bob Lib",
        "bob@example.invalid",
        "2026-01-04T10:00:00Z",
        "feat(fid-b): return 2 from b()",
        {
            "crates/fid-b/src/lib.rs": "pub fn b() -> u32 { 2 }\n",
        },
    ),
    # 5 (root README only)
    (
        "Carol Docs",
        "carol@example.invalid",
        "2026-01-05T10:00:00Z",
        "docs: add root README",
        {
            "README.md": "# Fixture Monorepo\n",
        },
    ),
    # 6 (touches fid-b #3)
    (
        "Carol Docs",
        "carol@example.invalid",
        "2026-01-06T10:00:00Z",
        "docs(fid-b): document b()",
        {
            "crates/fid-b/src/lib.rs": "/// Returns the fixture constant.\npub fn b() -> u32 { 2 }\n",
        },
    ),
    # 7 (fid-a only)
    (
        "Alice Dev",
        "alice@example.invalid",
        "2026-01-07T10:00:00Z",
        "test(fid-a): add comment",
        {
            "crates/fid-a/src/lib.rs": "// fid-a\npub fn a() -> u32 { fid_b::b() + 1 }\npub fn helper() -> u32 { 10 }\n",
        },
    ),
    # 8 (touches fid-b #4)
    (
        "Bob Lib",
        "bob@example.invalid",
        "2026-01-08T10:00:00Z",
        "feat(fid-b): add extra module",
        {
            "crates/fid-b/src/extra.rs": "pub fn extra() -> u32 { 40 }\n",
            "crates/fid-b/src/lib.rs": "pub mod extra;\n/// Returns the fixture constant.\npub fn b() -> u32 { 2 }\n",
        },
    ),
    # 9 (root only)
    (
        "Alice Dev",
        "alice@example.invalid",
        "2026-01-09T10:00:00Z",
        "docs: expand README",
        {
            "README.md": "# Fixture Monorepo\n\nContains fid-a and fid-b.\n",
        },
    ),
    # 10 (fid-a only)
    (
        "Alice Dev",
        "alice@example.invalid",
        "2026-01-10T10:00:00Z",
        "feat(fid-a): use extra",
        {
            "crates/fid-a/src/lib.rs": "// fid-a\npub fn a() -> u32 { fid_b::b() + fid_b::extra::extra() }\n",
        },
    ),
    # 11 (touches fid-b #5)
    (
        "Bob Lib",
        "bob@example.invalid",
        "2026-01-11T10:00:00Z",
        "feat(fid-b): bump extra to 42",
        {
            "crates/fid-b/src/extra.rs": "pub fn extra() -> u32 { 42 }\n",
        },
    ),
    # 12 (fid-a only)
    (
        "Carol Docs",
        "carol@example.invalid",
        "2026-01-12T10:00:00Z",
        "docs(fid-a): add crate README",
        {
            "crates/fid-a/README.md": "# fid-a\n",
        },
    ),
]


def _git(repo: Path, args: list[str], extra_env: dict[str, str] | None = None) -> str:
    env = {
        "PATH": os.environ.get("PATH", ""),
        "HOME": os.environ.get("HOME", ""),
        "LC_ALL": "C",
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_GLOBAL": os.devnull,
        "GIT_TERMINAL_PROMPT": "0",
    }
    if extra_env:
        env.update(extra_env)
    r = subprocess.run(
        ["git", "-C", str(repo), "-c", "core.autocrlf=false", "-c", "pack.threads=1", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
        check=True,
    )
    return r.stdout.strip()


def build_repo(repo_dir: Path) -> tuple[str, str, int, int]:
    repo_dir.mkdir(parents=True, exist_ok=True)
    _git(repo_dir, ["init", "-b", "main"])
    for name, email, date, msg, files in COMMITS:
        for rel, content in files.items():
            p = repo_dir / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(content.encode("utf-8"))
        _git(repo_dir, ["add", "-A"])
        _git(
            repo_dir,
            ["commit", "-m", msg],
            extra_env={
                "GIT_AUTHOR_NAME": name,
                "GIT_AUTHOR_EMAIL": email,
                "GIT_AUTHOR_DATE": date,
                "GIT_COMMITTER_NAME": name,
                "GIT_COMMITTER_EMAIL": email,
                "GIT_COMMITTER_DATE": date,
            },
        )
    head = _git(repo_dir, ["rev-parse", "HEAD"])
    fid_b_tree = _git(repo_dir, ["rev-parse", "HEAD:crates/fid-b"])
    total = int(_git(repo_dir, ["rev-list", "--count", "HEAD"]))
    fid_b_count = int(_git(repo_dir, ["rev-list", "--count", "HEAD", "--", "crates/fid-b"]))
    return head, fid_b_tree, total, fid_b_count


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify committed fixtures match this generator")
    args = ap.parse_args()

    with tempfile.TemporaryDirectory(prefix="promote-fix-") as td:
        src = Path(td) / "mono"
        head, fid_b_tree, total, fid_b_count = build_repo(src)
        assert total == 12, f"expected 12 commits, got {total}"
        assert fid_b_count == 5, f"expected 5 commits touching crates/fid-b, got {fid_b_count}"

        if args.check:
            if not REPOS_TOML.is_file() or REPOS_TOML.read_bytes() != REPOS_TOML_TEXT.encode("utf-8"):
                print("ERROR: fixtures/repo/tpl/promote/repos.toml does not match generator", file=sys.stderr)
                return 1
            if not BUNDLE.is_file():
                print("ERROR: fixtures/repo/tpl/promote/mono.bundle missing", file=sys.stderr)
                return 1
            clone = Path(td) / "clone"
            subprocess.run(
                ["git", "-c", "core.autocrlf=false", "clone", "-q", str(BUNDLE), str(clone)],
                check=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            c_head = _git(clone, ["rev-parse", "HEAD"])
            c_tree = _git(clone, ["rev-parse", "HEAD:crates/fid-b"])
            c_total = int(_git(clone, ["rev-list", "--count", "HEAD"]))
            c_b = int(_git(clone, ["rev-list", "--count", "HEAD", "--", "crates/fid-b"]))
            if (c_head, c_tree, c_total, c_b) != (head, fid_b_tree, total, fid_b_count):
                print(
                    f"ERROR: mono.bundle mismatch: got {(c_head, c_tree, c_total, c_b)} "
                    f"expected {(head, fid_b_tree, total, fid_b_count)}",
                    file=sys.stderr,
                )
                return 1
            return 0

        REPOS_TOML.write_bytes(REPOS_TOML_TEXT.encode("utf-8"))
        out_bundle = Path(td) / "mono.bundle"
        _git(src, ["bundle", "create", str(out_bundle), "HEAD", "main"])
        shutil.copyfile(out_bundle, BUNDLE)
        print(f"wrote {BUNDLE} (head={head}, crates/fid-b tree={fid_b_tree}, {total} commits, {fid_b_count} touching fid-b)")
        return 0


if __name__ == "__main__":
    sys.exit(main())
