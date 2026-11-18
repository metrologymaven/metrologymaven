#!/usr/bin/env python3
"""Keep one GitHub issue per expert-queue node, labelled by expertise.

Runs in CI (.github/workflows/expert-queue.yml) after every push to main that
touches the spec tree. It reads `tools/scope.py experts`, then with the `gh` CLI:

- creates the labels `expert-queue` and `expert:<area>` if missing;
- opens an issue for every queued node that has none;
- closes open queue issues whose node left the queue (expanded or no longer
  marked `needs_expert`).

It never edits an issue a person has commented on, other than closing it.
Usage:  python tools/sync_expert_issues.py [--dry-run]
"""

import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
REPO = os.environ.get("GITHUB_REPOSITORY", "metrologymaven/metrologymaven")
QUEUE_LABEL = "expert-queue"
COLORS = {"gdt": "b5132e", "uncertainty": "2f5f8a", "cmm-control": "a6620b"}


def gh(*args: str, dry: bool = False) -> str:
    if dry and args[:2] != ("issue", "list"):
        print("DRY gh", " ".join(a if len(a) < 80 else a[:60] + "..." for a in args))
        return ""
    r = subprocess.run(["gh", *args, "--repo", REPO], capture_output=True, text=True, check=False)
    if r.returncode != 0:
        raise SystemExit(f"gh {' '.join(args)} failed: {r.stderr.strip()}")
    return r.stdout


def title_for(item: dict) -> str:
    return f"[expert] {item['id']}: {item['title']}"


def body_for(item: dict) -> str:
    url = f"https://github.com/{REPO}/blob/main/{item['path']}"
    return f"""**Area:** {item['area_title']}  ·  **Phase:** {item['phase']}  ·  **Spec node:** [{item['id']}]({url})

This part of the open metrology platform needs someone with real expertise in **{item['area_title']}**. Agents have written the surrounding spec; this node is deliberately left for a human expert because getting it wrong would produce confidently wrong measurements.

### What we need

Read the node linked above. Then either:

1. **Expand it** yourself (split it into children or finish it as a leaf) following [`prompts/expand-node.md`](https://github.com/{REPO}/blob/main/prompts/expand-node.md), or
2. **Review an agent's draft**: comment with what is wrong, missing or ambiguous, in your own words, and an agent turns that into the PR for you to approve.

Never paste text from a standard. Cite the designator and clause number and explain in your own words.

### How to claim

Comment `/claim` on this issue. You'll be assigned automatically. Comment `/unclaim` to release it. Claims with no activity for 14 days are released.

### Credit

Expert contributors are listed by name in the node's `owners` field, in `CONTRIBUTORS.md` and in the release notes of every release that uses their work.

<sub>Generated from `generated/EXPERT-QUEUE.md` by `tools/sync_expert_issues.py`. Do not edit the title.</sub>
"""


def main() -> int:
    dry = "--dry-run" in sys.argv
    queue = json.loads(subprocess.run([sys.executable, str(ROOT / "tools" / "scope.py"), "experts"],
                                      capture_output=True, text=True, check=True).stdout)
    areas = {i["area"]: i["area_title"] for i in queue}

    gh("label", "create", QUEUE_LABEL, "--color", "15202b", "--force",
       "--description", "Spec nodes waiting for a human domain expert", dry=dry)
    for area, title in sorted(areas.items()):
        gh("label", "create", f"expert:{area}", "--color", COLORS.get(area, "6b7785"), "--force",
           "--description", title[:100], dry=dry)

    existing = json.loads(gh("issue", "list", "--label", QUEUE_LABEL, "--state", "all", "--limit", "1000",
                             "--json", "number,title,state") or "[]")
    by_id = {}
    for iss in existing:
        t = iss["title"]
        if t.startswith("[expert] ") and ":" in t:
            by_id[t[len("[expert] "):].split(":", 1)[0]] = iss

    wanted = {i["id"]: i for i in queue}
    created = closed = 0
    for nid, item in sorted(wanted.items()):
        if nid in by_id:
            continue
        gh("issue", "create", "--title", title_for(item), "--body", body_for(item),
           "--label", QUEUE_LABEL, "--label", f"expert:{item['area']}", dry=dry)
        created += 1
    for nid, iss in sorted(by_id.items()):
        if nid not in wanted and iss["state"] == "OPEN":
            gh("issue", "close", str(iss["number"]), "--comment",
               "This node left the expert queue (it was expanded or no longer needs an expert). Thank you!",
               dry=dry)
            closed += 1
    print(f"expert queue: {len(wanted)} nodes, {created} issues created, {closed} closed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
