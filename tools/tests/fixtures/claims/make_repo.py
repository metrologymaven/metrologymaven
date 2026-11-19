#!/usr/bin/env python3
"""Build the ECO.COMM.CLAIM fixture repository with fixed commit dates.

    python tools/tests/fixtures/claims/make_repo.py <empty-dir> [--bundle <out.bundle>]

History (all dates UTC; the tests use --now 2026-12-01T00:00:00Z):
  2026-10-01  nodes A, B, C, D, E, F created, all unclaimed
  2026-10-20  F claimed by alice2                              (superseded below)
  2026-11-11  A claimed by alice, C claimed by carol           (20 days before --now)
  2026-11-17  E claimed by erin                                (exactly 14 days: not stale)
  2026-11-20  A's body edited, claim untouched                 (claim date must stay 11-11)
  2026-11-25  F reassigned alice2 -> alice                     (6 days: claim date is 11-25,
                                                               not 10-20, so F is not stale)
  2026-11-28  B claimed by bob                                 (3 days before --now)
D stays unclaimed. Expected stale list (hand-labelled): A only, C being named by open PR #7.
"""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

NODES = ["A", "B", "C", "D", "E", "F"]


def node_text(nid: str, claimed: str = "", body: str = "Fixture node.") -> str:
    return (
        "---\n"
        f"id: {nid}\n"
        f"title: Fixture node {nid}\n"
        "level: 1\n"
        "parent: ROOT\n"
        "kind: task\n"
        "status: leaf\n"
        "phase: P0\n"
        "size: S\n"
        f"claimed_by:{(' ' + claimed) if claimed else ''}\n"
        "---\n"
        "\n"
        f"# {nid}\n"
        "\n"
        f"{body}\n"
    )


def write(repo: Path, nid: str, text: str) -> None:
    p = repo / "spec" / nid.lower() / "NODE.md"
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)


def git(repo: Path, *args: str, date: str = "") -> None:
    env = dict(os.environ)
    env.update({
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_AUTHOR_NAME": "Fixture",
        "GIT_AUTHOR_EMAIL": "fixture@example.invalid",
        "GIT_COMMITTER_NAME": "Fixture",
        "GIT_COMMITTER_EMAIL": "fixture@example.invalid",
    })
    if date:
        env["GIT_AUTHOR_DATE"] = date
        env["GIT_COMMITTER_DATE"] = date
    cfg = ["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false", "-c", "init.defaultBranch=main"]
    subprocess.run(["git", "-C", str(repo), *cfg, *args], check=True, env=env,
                   stdout=subprocess.DEVNULL)


def commit(repo: Path, msg: str, date: str) -> None:
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", msg, date=date)


def build(repo: Path) -> None:
    repo.mkdir(parents=True, exist_ok=True)
    git(repo, "init", "-q")
    for n in NODES:
        write(repo, n, node_text(n))
    commit(repo, "seed nodes", "2026-10-01T12:00:00+00:00")
    write(repo, "F", node_text("F", "alice2"))
    commit(repo, "claim F", "2026-10-20T08:00:00+00:00")
    write(repo, "A", node_text("A", "alice"))
    write(repo, "C", node_text("C", "carol"))
    commit(repo, "claim A and C", "2026-11-11T09:00:00+00:00")
    write(repo, "E", node_text("E", "erin"))
    commit(repo, "claim E", "2026-11-17T00:00:00+00:00")
    write(repo, "A", node_text("A", "alice", "Fixture node, body edited later."))
    commit(repo, "edit A body", "2026-11-20T10:00:00+00:00")
    write(repo, "F", node_text("F", "alice"))
    commit(repo, "reassign F", "2026-11-25T08:00:00+00:00")
    write(repo, "B", node_text("B", "bob"))
    commit(repo, "claim B", "2026-11-28T15:30:00+00:00")


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__, file=sys.stderr)
        return 2
    repo = Path(argv[0])
    build(repo)
    if len(argv) == 3 and argv[1] == "--bundle":
        git(repo, "bundle", "create", str(Path(argv[2]).resolve()), "--all")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
