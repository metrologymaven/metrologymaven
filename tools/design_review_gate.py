#!/usr/bin/env python3
"""CI wrapper of the design-review gate (spec node UX.SYSTEM.REVIEW).

`fid-design review-check` cannot verify who approved: a record's `approver.name` is text. This
wrapper backs it with GitHub. Using read-only `gh api` GET calls it:

1. reads the PR's head commit and its changed files;
2. runs `fid-design review-check --changed <list> --root <design dir> [--base .] --pr <N> --json`;
3. reads the PR's reviews and keeps, per reviewer, the latest review. A reviewer counts when
   that review is `APPROVED`, was given on the head commit, and the account is not a bot
   (`type` other than `Bot`, login not ending in `[bot]`);
4. requires every valid record's `approver.name` to equal (ASCII case-insensitively) the login
   of such a reviewer. Otherwise it reports `REVIEW-APPROVER-UNVERIFIED <record>`.

It changes nothing on GitHub and never reads or prints a token; `gh` takes its own credentials
from the environment. It cannot tell a person from an agent that uses that person's account.

Usage (from the repository root):

    python tools/design_review_gate.py --repo <owner/name> --pr <N> [--root design] [--base]
        [--fid-design <path to the binary>]

Without `--base`, changed paths outside the design directory are dropped and the rest are made
relative to it. With `--base`, paths stay relative to the repository root, so the map can govern
UI folders outside the design directory. Exit codes: 0 pass, 1 findings, 2 error.
Python stdlib only.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile

UNVERIFIED = "REVIEW-APPROVER-UNVERIFIED"


def gh_get(path):
    """One read-only `gh api` GET, with pagination; returns the parsed JSON."""
    r = subprocess.run(
        ["gh", "api", "--method", "GET", "--paginate", "--slurp", path],
        capture_output=True,
        text=True,
        check=False,
    )
    if r.returncode != 0:
        raise RuntimeError(f"gh api {path} failed: {r.stderr.strip()}")
    pages = json.loads(r.stdout)
    if pages and isinstance(pages[0], list):
        return [x for page in pages for x in page]
    return pages[0] if len(pages) == 1 else pages


def is_bot(user):
    login = (user or {}).get("login") or ""
    return (user or {}).get("type") == "Bot" or login.lower().endswith("[bot]") or not login


def approved_logins(reviews, head_sha):
    """ASCII-lowercased logins whose latest state-changing review approves `head_sha`."""
    latest = {}
    for r in reviews:
        user = r.get("user") or {}
        login = (user.get("login") or "").lower()
        if not login or r.get("state") == "COMMENTED":
            continue  # a comment does not change a reviewer's approval
        latest[login] = r
    out = set()
    for login, r in latest.items():
        if r.get("state") == "APPROVED" and r.get("commit_id") == head_sha and not is_bot(r.get("user")):
            out.add(login)
    return out


def changed_paths(files, root_rel, use_base):
    """The changed-path list for review-check, from the PR's `files` entries."""
    out = set()
    prefix = root_rel.strip("/") + "/"
    for f in files:
        for name in (f.get("filename"), f.get("previous_filename")):
            if not name:
                continue
            if use_base:
                out.add(name)
            elif name.startswith(prefix):
                out.add(name[len(prefix):])
    return sorted(out)


def verify(report, approved):
    """Findings for records whose approver is not a verified GitHub approver."""
    out = []
    for a in report.get("approvals", []):
        if a.get("approver", "").lower() not in approved:
            out.append(f"{UNVERIFIED} {a.get('record')}: approver is not a non-bot GitHub approver of the head commit")
    return out


def run(args, gh=gh_get):
    pr = gh(f"repos/{args.repo}/pulls/{args.pr}")
    head = pr["head"]["sha"]
    files = gh(f"repos/{args.repo}/pulls/{args.pr}/files")
    paths = changed_paths(files, args.root, args.base)
    if not paths:
        print("design review: no changed path under the design directory")
        return 0
    with tempfile.TemporaryDirectory() as tmp:
        lst = os.path.join(tmp, "changed.txt")
        with open(lst, "w", encoding="utf-8", newline="\n") as fh:
            fh.write("".join(p + "\n" for p in paths))
        cmd = [args.fid_design, "review-check", "--changed", lst, "--root", args.root, "--pr", str(args.pr), "--json"]
        if args.base:
            cmd += ["--base", "."]
        r = subprocess.run(cmd, capture_output=True, text=True, check=False)
    if r.returncode not in (0, 1):
        print(r.stderr, file=sys.stderr)
        return 2
    report = json.loads(r.stdout)
    for f in report.get("findings", []):
        print(f"{f['code']} {f['subject']}: {f['detail']}")
    extra = verify(report, approved_logins(gh(f"repos/{args.repo}/pulls/{args.pr}/reviews"), head))
    for line in extra:
        print(line)
    return 1 if (r.returncode == 1 or extra) else 0


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--repo", required=True)
    p.add_argument("--pr", required=True, type=int)
    p.add_argument("--root", default="design")
    p.add_argument("--base", action="store_true", help="paths relative to the repository root")
    p.add_argument("--fid-design", default="fid-design")
    args = p.parse_args(argv)
    if args.pr < 1:
        p.error("--pr must be positive")
    try:
        return run(args)
    except (RuntimeError, KeyError, ValueError) as e:
        print(f"design review: error: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
