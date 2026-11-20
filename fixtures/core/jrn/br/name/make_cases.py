#!/usr/bin/env python3
"""Independent oracle for CORE.JRN.BR.NAME (branch create, list, protection, co-editing).

Written from the rules of spec/core/jrn/br/name/NODE.md with the Python standard library
only; nothing here is produced by running the Rust code. Writes cases.json (CC-BY-4.0):

  project   the synthetic 5-entry project of AC-1: entry i (1..5) has hash
            sha256("br-name-entry-<i>"); main's head is entry 5; those 5 hashes are the
            project's known heads.
  names     branch names with the expected verdict and owner. Rules: the spec grammar
            [a-z0-9][a-z0-9/_.-]{0,99} (ASCII only, so no Unicode look-alike passes), plus no
            empty, "." or ".." path segment, no trailing "/", and the name "agent" refused;
            owner "agent" for the agent/ namespace, else "human".
  scenario  ordered calls with the caller kind (human, agent, script) and the expected
            result: the event to commit (type and body), "noop" (nothing to commit), or a bus
            error (code, reason; catalogue v3). Followed by the expected branch list.
  tamper    event logs that a correct journal never holds, each with the expected refusal
            (internal/invariant_violated), plus their position in the log.
  release   REQ-006: which (branch, class) commits add BranchProtected{main, release}.

Rules per command (the bus derives the caller from the principal, never from parameters):
  create_branch    bad name / agent/ namespace / agent caller / existing name ->
                   validation/bad_branch_name; unknown head -> not_found/unknown_entity;
                   else BranchCreated{name, from}. Checked in that order.
  protect_branch   non-human -> policy/class_forbidden; unknown -> not_found/unknown_branch;
                   protected already -> noop; else BranchProtected{name, cause: manual}.
  set_coediting    non-human -> policy/human_only; unknown -> not_found/unknown_branch; agent
                   branch -> validation/bad_branch_name; bad session -> validation/schema; no
                   change -> noop; else CoeditingSet{name, agent_session, allow}.
  agent_branch     (the bus routing module) agent/<client>/<n>; existing -> bad_branch_name.

Run:   python fixtures/core/jrn/br/name/make_cases.py
Check: python fixtures/core/jrn/br/name/make_cases.py --check
"""

import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.json"
GRAMMAR = re.compile(r"[a-z0-9][a-z0-9/_.\-]{0,99}", re.ASCII)
SESSION = re.compile(r"[A-Za-z0-9][A-Za-z0-9._:\-]{0,127}", re.ASCII)
CLIENT = re.compile(r"[a-z0-9][a-z0-9_.\-]{0,62}", re.ASCII)


def sha(text: str) -> str:
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


ENTRIES = [sha(f"br-name-entry-{i}") for i in range(1, 6)]
HEAD = ENTRIES[-1]


def name_ok(n: str) -> bool:
    if GRAMMAR.fullmatch(n) is None:
        return False
    if n.endswith("/") or any(seg in ("", ".", "..") for seg in n.split("/")):
        return False
    return n != "agent"


def owner(n: str) -> str:
    return "agent" if n.startswith("agent/") else "human"


NAMES = [
    "main", "trial/a", "a", "0", "release-1.2", "x_y", "a/b/c", "a.b", "agent/claude/1",
    "Trial A", "trial a", "TRIAL", "-a", "/a", "a/", "a//b", "a/./b", "a/../b", "..", ".a", "_a",
    "agent", "agent/", "a" * 100, "a" * 101, "", "triаl",  # Cyrillic a
    "ｔrial", "trial​", "trial\n", "café", "café", "trial‐a", "maın",
    "a\\b", "a:b", "a@b", "a b",
]


def name_cases():
    return [{"name": n, "ok": name_ok(n), "owner": owner(n) if name_ok(n) else None} for n in NAMES]


class Table:
    def __init__(self):
        self.b = {"main": {"head": HEAD, "created_from": None, "protected": False, "coediting": set()}}

    def known_head(self, h):
        return h in ENTRIES or any(v["head"] == h or v["created_from"] == h for v in self.b.values())

    def apply(self, ev):
        t, body = ev["type"], ev["body"]
        n = body["name"]
        if t == "BranchCreated":
            assert n not in self.b
            self.b[n] = {"head": body["from"], "created_from": body["from"], "protected": False, "coediting": set()}
        elif t == "BranchProtected":
            assert not self.b[n]["protected"]
            self.b[n]["protected"] = True
        else:
            s = self.b[n]["coediting"]
            (s.add if body["allow"] else s.discard)(body["agent_session"])

    def listing(self):
        return [
            {
                "name": n,
                "head": v["head"],
                "created_from": v["created_from"],
                "protected": v["protected"],
                "owner": owner(n),
                "coediting": sorted(v["coediting"]),
            }
            for n, v in sorted(self.b.items(), key=lambda kv: kv[0].encode("utf-8"))
        ]


def err(code, reason):
    return {"error": {"code": code, "reason": reason}}


def call(t: Table, op: dict):
    kind, actor = op["op"], op["actor"]
    if kind == "create":
        n, frm = op["name"], op["from"]
        if not name_ok(n) or owner(n) == "agent" or actor == "agent" or n in t.b:
            return err("validation", "bad_branch_name")
        if not t.known_head(frm):
            return err("not_found", "unknown_entity")
        return {"event": {"type": "BranchCreated", "body": {"from": frm, "name": n}}}
    if kind == "protect":
        if actor != "human":
            return err("policy", "class_forbidden")
        n = op["branch"]
        if n not in t.b:
            return err("not_found", "unknown_branch")
        if t.b[n]["protected"]:
            return {"noop": True}
        return {"event": {"type": "BranchProtected", "body": {"cause": "manual", "name": n}}}
    if kind == "coediting":
        if actor != "human":
            return err("policy", "human_only")
        n, s, allow = op["branch"], op["agent_session"], op["allow"]
        if n not in t.b:
            return err("not_found", "unknown_branch")
        if owner(n) == "agent":
            return err("validation", "bad_branch_name")
        if SESSION.fullmatch(s) is None:
            return err("validation", "schema")
        if (s in t.b[n]["coediting"]) == allow:
            return {"noop": True}
        return {"event": {"type": "CoeditingSet", "body": {"agent_session": s, "allow": allow, "name": n}}}
    if kind == "agent_branch":
        c, k = op["client"], op["n"]
        n = f"agent/{c}/{k}"
        if CLIENT.fullmatch(c) is None or c in (".", "..") or not name_ok(n) or n in t.b:
            return err("validation", "bad_branch_name")
        return {"event": {"type": "BranchCreated", "body": {"from": op["from"], "name": n}}}
    raise ValueError(kind)


OPS = [
    # AC-1
    {"op": "create", "actor": "human", "name": "trial/a", "from": HEAD},
    # AC-2
    {"op": "create", "actor": "human", "name": "trial/a", "from": HEAD},
    {"op": "create", "actor": "human", "name": "Trial A", "from": HEAD},
    {"op": "create", "actor": "script", "name": "agent/x/1", "from": HEAD},
    {"op": "create", "actor": "human", "name": "agent/x/1", "from": HEAD},
    # Agents never name a human branch.
    {"op": "create", "actor": "agent", "name": "trial/b", "from": HEAD},
    {"op": "create", "actor": "agent", "name": "agent/x/2", "from": HEAD},
    {"op": "create", "actor": "human", "name": "triаl/b", "from": HEAD},
    {"op": "create", "actor": "human", "name": "main", "from": HEAD},
    {"op": "create", "actor": "human", "name": "trial/c", "from": sha("not-an-entry")},
    {"op": "create", "actor": "script", "name": "trial/c", "from": ENTRIES[1]},
    # The routing module's agent branch.
    {"op": "agent_branch", "actor": "routing", "client": "claude", "n": 1, "from": HEAD},
    {"op": "agent_branch", "actor": "routing", "client": "claude", "n": 1, "from": HEAD},
    {"op": "agent_branch", "actor": "routing", "client": "Claude", "n": 2, "from": HEAD},
    # AC-3
    {"op": "protect", "actor": "agent", "branch": "main"},
    {"op": "protect", "actor": "script", "branch": "main"},
    {"op": "protect", "actor": "human", "branch": "nope"},
    {"op": "protect", "actor": "human", "branch": "main"},
    {"op": "protect", "actor": "human", "branch": "main"},
    # AC-5
    {"op": "coediting", "actor": "human", "branch": "main", "agent_session": "S", "allow": True},
    {"op": "coediting", "actor": "human", "branch": "main", "agent_session": "S", "allow": True},
    {"op": "coediting", "actor": "script", "branch": "main", "agent_session": "S", "allow": True},
    {"op": "coediting", "actor": "agent", "branch": "main", "agent_session": "S2", "allow": True},
    {"op": "coediting", "actor": "human", "branch": "agent/claude/1", "agent_session": "S", "allow": True},
    {"op": "coediting", "actor": "human", "branch": "main", "agent_session": "bad session", "allow": True},
    {"op": "coediting", "actor": "human", "branch": "trial/a", "agent_session": "S2", "allow": True},
    {"op": "coediting", "actor": "human", "branch": "trial/a", "agent_session": "S2", "allow": False},
    {"op": "coediting", "actor": "human", "branch": "trial/a", "agent_session": "S2", "allow": False},
    {"op": "coediting", "actor": "human", "branch": "main", "agent_session": "S3", "allow": True},
]


def scenario():
    t = Table()
    out, log = [], []
    for op in OPS:
        res = call(t, op)
        if "event" in res:
            t.apply(res["event"])
            log.append(res["event"])
        out.append({"call": op, "expect": res})
    return out, log, t.listing()


def ev(t, **body):
    return {"type": t, "body": body}


def tamper_cases():
    c = lambda n, f=HEAD: ev("BranchCreated", **{"from": f, "name": n})  # noqa: E731
    p = lambda n, cause="manual": ev("BranchProtected", cause=cause, name=n)  # noqa: E731
    s = lambda n, sess, allow: ev("CoeditingSet", agent_session=sess, allow=allow, name=n)  # noqa: E731
    bad = [
        ("duplicate create", [c("trial/a"), c("trial/a")], 1),
        ("create main again", [c("main")], 0),
        ("protect twice", [p("main"), p("main")], 1),
        ("protect unknown", [p("nope")], 0),
        ("release protection off main", [c("trial/a"), p("trial/a", "release")], 1),
        ("unknown cause", [p("main", "admin")], 0),
        ("coediting no-op", [s("main", "S", False)], 0),
        ("coediting twice", [s("main", "S", True), s("main", "S", True)], 1),
        ("coediting on agent branch", [c("agent/claude/1"), s("agent/claude/1", "S", True)], 1),
        ("coediting unknown branch", [s("nope", "S", True)], 0),
        ("bad name", [c("Trial A")], 0),
        ("confusable name", [c("triаl")], 0),
        ("bad digest", [c("trial/a", "sha256:XYZ")], 0),
        ("extra key", [ev("BranchCreated", **{"from": HEAD, "name": "x", "owner": "human"})], 0),
        ("missing key", [ev("BranchProtected", name="main")], 0),
        ("unknown type", [ev("BranchDeleted", name="main")], 0),
        ("unprotect", [p("main"), ev("BranchUnprotected", name="main")], 1),
        ("allow not bool", [ev("CoeditingSet", agent_session="S", allow="true", name="main")], 0),
        ("name not string", [ev("BranchProtected", cause="manual", name=7)], 0),
    ]
    return [{"why": w, "log": log, "at": at} for w, log, at in bad]


def release_cases():
    return [
        {"branch": "main", "class": "W3", "protected_before": False, "adds": True},
        {"branch": "main", "class": "W3", "protected_before": True, "adds": False},
        {"branch": "main", "class": "W2", "protected_before": False, "adds": False},
        {"branch": "trial/a", "class": "W3", "protected_before": False, "adds": False},
    ]


def build() -> str:
    calls, log, listing = scenario()
    doc = {
        "about": "Independent oracle for CORE.JRN.BR.NAME, generated by make_cases.py from the spec rules "
        "with the Python standard library; never produced by the Rust code. CC-BY-4.0.",
        "project": {"entries": ENTRIES, "main_head": HEAD},
        "names": name_cases(),
        "scenario": {"calls": calls, "log": log, "branches": listing},
        "tamper": tamper_cases(),
        "release": release_cases(),
    }
    return json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"


def main() -> int:
    text = build()
    if "--check" in sys.argv[1:]:
        have = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if have != text:
            print(f"{OUT} is stale; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print(f"{OUT.name} up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
