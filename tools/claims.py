#!/usr/bin/env python3
"""Claim staleness and code-to-node traceability (spec node ECO.COMM.CLAIM).

    python tools/claims.py stale --repo <dir> --prs <prs.json> --now <ISO8601> [--json]
    python tools/claims.py trace --pr <pr.json> --tree <spec-dir>

Standard library and `git` only. PR data always comes from a JSON file (an export such as
`gh pr list --json number,title,body,headRefName,labels,state`); this tool never calls the forge.
Output is sorted, byte-stable JSON. Exit 0 on success, 1 on a failed check or an error
(`CLAIMS-GIT`, `CLAIMS-INPUT`).
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

from scope import parse_front  # noqa: E402  (SCOPE's node parser; ECO.COMM.SCOPE)

STALE_AFTER = timedelta(days=14)
MAX_INPUT_BYTES = 64 * 1024 * 1024
OK_STATUSES = {"leaf", "frozen"}
EXEMPT_LABEL = "no-spec"
IMPLEMENTS_RE = re.compile(r"^[ \t]*Implements:[ \t]*([A-Z0-9.]+)", re.MULTILINE)
CLAIM_LINE_RE = re.compile(r"^claimed_by:(.*)$", re.MULTILINE)


class ClaimsError(Exception):
    def __init__(self, code: str, msg: str) -> None:
        super().__init__(f"{code} {msg}")
        self.code = code


def emit(obj) -> None:
    data = json.dumps(obj, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
    sys.stdout.buffer.write(data.encode("utf-8"))
    sys.stdout.flush()


def parse_time(s: str) -> datetime:
    t = s.strip()
    if t.endswith("Z") or t.endswith("z"):
        t = t[:-1] + "+00:00"
    try:
        dt = datetime.fromisoformat(t)
    except ValueError as e:
        raise ClaimsError("CLAIMS-INPUT", f"bad timestamp {s!r}") from e
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc)


def fmt_time(dt: datetime) -> str:
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def read_text(path, what: str = "") -> str:
    """Read a UTF-8 input file of at most MAX_INPUT_BYTES; any failure is CLAIMS-INPUT."""
    try:
        if os.path.getsize(path) > MAX_INPUT_BYTES:
            raise ClaimsError("CLAIMS-INPUT", f"{path}: larger than {MAX_INPUT_BYTES} bytes")
        with open(path, "rb") as f:
            return f.read(MAX_INPUT_BYTES + 1).decode("utf-8")
    except (OSError, UnicodeDecodeError) as e:
        raise ClaimsError("CLAIMS-INPUT", f"{what or path}: {e}") from e


def load_json(path: str):
    text = read_text(path)
    try:
        return json.loads(text)
    except (ValueError, RecursionError) as e:
        raise ClaimsError("CLAIMS-INPUT", f"{path}: {e}") from e


def _text(pr: dict, *keys: str) -> str:
    """The first present key's value; it must be a string or null."""
    for k in keys:
        if k in pr:
            v = pr[k]
            if v is None:
                return ""
            if not isinstance(v, str):
                raise ClaimsError("CLAIMS-INPUT", f"PR field {k!r} is not a string")
            return v
    return ""


def norm_pr(pr) -> dict:
    """Accept both the spec's field names and `gh`'s (headRefName, labels as objects)."""
    if not isinstance(pr, dict):
        raise ClaimsError("CLAIMS-INPUT", "PR entry is not an object")
    raw_labels = pr.get("labels")
    if raw_labels is None:
        raw_labels = []
    if not isinstance(raw_labels, list):
        raise ClaimsError("CLAIMS-INPUT", "PR field 'labels' is not a list")
    labels = []
    for lab in raw_labels:
        name = lab.get("name") if isinstance(lab, dict) else lab
        if not isinstance(name, str):
            raise ClaimsError("CLAIMS-INPUT", "PR label is neither a string nor an object with a string 'name'")
        labels.append(name)
    number = pr.get("number")
    if number is not None and (isinstance(number, bool) or not isinstance(number, int)):
        raise ClaimsError("CLAIMS-INPUT", "PR field 'number' is not an integer")
    return {
        "number": number,
        "title": _text(pr, "title"),
        "body": _text(pr, "body"),
        "head_ref": _text(pr, "head_ref", "headRefName"),
        "labels": sorted(set(labels)),
        "state": _text(pr, "state") or "OPEN",
    }


def mentions(node_id: str, text: str) -> bool:
    """Word-boundary match that treats `.` as part of an ID: `A.B` does not match `A.B.C`."""
    pat = r"(?<![A-Za-z0-9.])" + re.escape(node_id) + r"(?![A-Za-z0-9]|\.[A-Za-z0-9])"
    return re.search(pat, text) is not None


def load_tree(tree: Path) -> dict[str, dict]:
    """Map node ID -> {"meta", "path", "text"} for every NODE.md under `tree`."""
    if not tree.is_dir():
        raise ClaimsError("CLAIMS-INPUT", f"{tree}: spec tree is not a directory")
    nodes: dict[str, dict] = {}
    for p in sorted(tree.rglob("NODE.md")):
        text = read_text(p)
        meta, _body = parse_front(text)
        if meta is None or not meta.get("id"):
            continue
        nodes[meta["id"]] = {"meta": meta, "path": p, "text": text}
    return nodes


def git(repo: Path, *args: str) -> str:
    try:
        r = subprocess.run(
            ["git", "-C", str(repo), *args],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
        )
    except OSError as e:
        raise ClaimsError("CLAIMS-GIT", f"git unavailable: {e}") from e
    if r.returncode != 0:
        raise ClaimsError("CLAIMS-GIT", r.stderr.decode("utf-8", "replace").strip())
    return r.stdout.decode("utf-8", "replace")


def posix_literal(s: str) -> str:
    """`s` as a POSIX regex (same meaning in BRE and ERE) matching itself literally."""
    out = []
    for ch in s:
        if ch.isalnum() or ch in " _-@,:/'\"<>=!%&~`#;":
            out.append(ch)
        elif ch == "^":
            out.append("\\^")
        else:
            out.append(f"[{ch}]")
    return "".join(out)


def claim_regex(value: str) -> str:
    """Matches a diff line that sets `claimed_by:` to exactly `value` (cf. git log -G)."""
    return "^claimed_by:[ \t]*" + posix_literal(value) + "[ \t\r]*$"


def claim_value(text: str) -> str:
    """The raw value of the frontmatter `claimed_by:` line, or '' when absent or empty."""
    end = text.find("\n---", 3)
    front = text[: end if end > 0 else len(text)]
    m = CLAIM_LINE_RE.search(front.replace("\r\n", "\n"))
    return m.group(1).strip() if m else ""


def cmd_stale(args) -> int:
    repo = Path(args.repo).resolve()
    now = parse_time(args.now) if args.now else datetime.now(timezone.utc)
    raw = load_json(args.prs)
    if not isinstance(raw, list):
        raise ClaimsError("CLAIMS-INPUT", f"{args.prs}: expected a JSON list of PRs")
    open_texts = [
        "\n".join((p["title"], p["body"], p["head_ref"]))
        for p in map(norm_pr, raw)
        if p["state"].upper() == "OPEN"
    ]
    git(repo, "rev-parse", "--git-dir")
    stale = []
    spec = (repo / args.spec).resolve()
    if spec != repo and repo not in spec.parents:
        raise ClaimsError("CLAIMS-INPUT", f"{args.spec}: spec tree is not inside --repo")
    for nid, n in sorted(load_tree(spec).items()):
        value = claim_value(n["text"])
        if not value:
            continue
        rel = n["path"].resolve().relative_to(repo).as_posix()
        # -G (not -S): a commit counts only if it adds or removes a line whose whole value is
        # `value`, so an earlier claim by e.g. "alice2" never dates a later claim by "alice".
        out = git(repo, "log", "-1", "--format=%cI", "-G", claim_regex(value), "--", rel).strip()
        if not out:
            print(f"warn  {rel}: claim {value!r} not found in history", file=sys.stderr)
            continue
        claimed_at = parse_time(out)
        if now - claimed_at <= STALE_AFTER:
            continue
        if any(mentions(nid, t) for t in open_texts):
            continue
        stale.append({"id": nid, "claimed_by": value, "claimed_at": fmt_time(claimed_at)})
    if args.json:
        emit({"stale": stale})
    else:
        for s in stale:
            print(f"{s['id']}\t{s['claimed_by']}\t{s['claimed_at']}")
    return 0


def cmd_trace(args) -> int:
    pr = norm_pr(load_json(args.pr))
    tree = load_tree(Path(args.tree))
    named = sorted({m.rstrip(".") for t in (pr["title"], pr["body"]) for m in IMPLEMENTS_RE.findall(t)} - {""})
    exempt = EXEMPT_LABEL if EXEMPT_LABEL in pr["labels"] else None
    codes = []
    if not named and exempt is None:
        codes.append("TRACE-NONE")
    for nid in named:
        n = tree.get(nid)
        if n is None:
            codes.append(f"TRACE-UNKNOWN {nid}")
        elif n["meta"].get("status") not in OK_STATUSES:
            codes.append(f"TRACE-NOT-LEAF {nid}")
    codes.sort()
    for c in codes:
        print(c, file=sys.stderr)
    emit({
        "pr": pr["number"],
        "nodes": named,
        "result": "fail" if codes else "ok",
        "codes": codes,
        "exempt": exempt,
    })
    return 1 if codes else 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="claims.py", description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("stale", help="list claims older than 14 days with no open PR")
    s.add_argument("--repo", required=True, help="git checkout of the spec repository")
    s.add_argument("--prs", required=True, help="JSON list of PRs [{number,title,body,head_ref,labels,state}]")
    s.add_argument("--now", help="reference time (ISO 8601); CI always supplies it")
    s.add_argument("--spec", default="spec", help="spec tree inside --repo (default: spec)")
    s.add_argument("--json", action="store_true", help="emit JSON")
    s.set_defaults(fn=cmd_stale)
    t = sub.add_parser("trace", help="check a code PR names a spec leaf with `Implements: <ID>`")
    t.add_argument("--pr", required=True, help="JSON object of one PR")
    t.add_argument("--tree", required=True, help="spec tree at the pinned revision")
    t.set_defaults(fn=cmd_trace)
    args = ap.parse_args(argv)
    try:
        return args.fn(args)
    except ClaimsError as e:
        print(str(e), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
