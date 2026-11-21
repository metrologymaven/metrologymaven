#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.BR.PROMO (branch promotion and promotion requests).

An independent reference written with Python's standard library only. It reuses the
independent BR.DIFF reference (fixtures/core/jrn/br/diff/make_fixtures.py: its own RFC 8785
writer, header check and diff) to compute the verified diff a promotion is bound to, and adds
its own reading of the promotion rules of spec/core/jrn/br/promo. Nothing here is produced by,
or copied from, the Rust crates; the Rust tests load these files and must reach the same
outcome.

Files written next to this script (CC-BY-4.0):

- journals.json: three journals of stream `record` (rows with stored header bytes) and the
  branch table (name, created_from, protected) they share:
  `ff` (the target `main` is an ancestor of `trial` and of `agent/claude/1`), `fork` (`ff`
  plus one entry on `main`, so `trial` no longer fast-forwards), and `after_ff` (`ff` plus the
  entry a committed fast-forward of `trial` into `main` leaves on `main`).
- cases.json: `plans`, calls of the three commands (journal, command, caller principal kind,
  parameters, the fake Rebaser's steps or none, whether an AssertionCheck is wired and which
  assertion it confirms) with the expected event or refusal; and `folds`, event logs (each
  entry's origin, whether it carries a verified human assertion, and its events) with the
  expected outcome of folding them in order.

Plan rules, checked in this order (the first refusal wins):

 1. Caller: promote_branch needs a human (else policy/human_only); promote_protected_branch a
    human (else policy/class_forbidden); request_promote_branch anyone.
 2. A request's note is at most 2000 characters (else validation/schema).
 3. Source, then target, must be in the branch table (else not_found/unknown_branch).
 4. Source equal to target, or a target in the agent/ namespace: precondition/false.
 5. promote_branch into a protected target: policy/target_protected; promote_protected_branch
    into an unprotected one: precondition/false.
 6. The verified diff of source against target (BR.DIFF). Its source head must equal
    `source_head` and its target head `target_head` (null for the empty state), else
    conflict/expected_head_mismatch; SHA-256 of its canonical bytes must equal `diff`, else
    conflict/expected_digest_mismatch.
 7. Nothing to promote (source head equal to target head, or the ancestor is the source head):
    precondition/false.
 8. A request: event PromotionRequested {diff, note?, source, source_head, target,
    target_head}; no head moves.
 9. promote_protected_branch: the pre-image is SHA-256 of the canonical
    {diff, source, source_head, target, target_head}; no AssertionCheck wired:
    precondition/capability_missing; not confirmed: policy/no_assertion.
10. Fast-forward when the ancestor equals the target head: mode fast_forward, no entries, no
    Rebaser call.
11. Otherwise the Rebaser: none wired: precondition/capability_missing; no steps:
    precondition/false; more than 10000: validation/input_limit; the first step whose outcome
    is not IDENTICAL or EQUIVALENT: precondition/divergent_step naming it (1-based); else
    mode rebased with every step as an entry {command, digest, outcome, step}.
12. Event BranchPromoted (promote_branch) or ProtectedBranchPromoted (plus pre_image)
    {diff, entries, mode, source, source_head, target, target_head}: one event, so the
    promotion lands whole or not at all.

Fold rules: an entry holds at most one promotion event. BranchPromoted only from
journal.promote_branch@1's handler, a human principal, class W2, on an entry of the target
branch, into a human-owned unprotected target; ProtectedBranchPromoted only from
journal.promote_protected_branch@1's handler, a human principal, class W3, an entry carrying
a verified human assertion, on the target branch, into a protected target, with the pre_image
recomputed from the body; PromotionRequested only from journal.request_promote_branch@1's
handler, class W1, any principal. Bodies have exactly their keys; source and target are known,
distinct branches; fast_forward has no entries, rebased 1 to 10000 entries numbered from 1
whose outcomes are IDENTICAL or EQUIVALENT. A promotion of the same (target, target_head,
source_head) a second time is a replay. Anything else is internal/invariant_violated.

Run:  python fixtures/core/jrn/br/promo/make_fixtures.py [--check]
"""

import copy
import importlib.util
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("brdiff", HERE.parent / "diff" / "make_fixtures.py")
bd = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(bd)

canon, cbytes, sha, val = bd.canon, bd.cbytes, bd.sha, bd.val
STREAM = bd.STREAM
ZERO = bd.ZERO
MAX_STEPS = 10000
MAX_NOTE = 2000
PROMOTE = "journal.promote_branch"
PROTECTED = "journal.promote_protected_branch"
REQUEST = "journal.request_promote_branch"
OK_OUTCOMES = ("IDENTICAL", "EQUIVALENT")

# seq: (branch, principal, kind, command, logical parent seq or None, outputs)
PLAN = [
    (1, "main", "human", "session_header", None, None, []),
    (2, "main", "human", "command", "project.add_features", None, [("e1", val("e1", 1)), ("e2", val("e2", 1))]),
    (3, "trial", "agent", "command", "feature.add", 2, [("e3", val("e3", 1))]),
    (4, "trial", "agent", "command", "feature.set_nominal", 3, [("e1", val("e1", 2))]),
    (5, "agent/claude/1", "agent", "command", "feature.set_nominal", 2, [("e2", val("e2", 2))]),
    (6, "rel", "human", "command", "feature.add", 2, [("e5", val("e5", 1))]),
]
FORK_EXTRA = (7, "main", "human", "command", "feature.set_nominal", 2, [("e2", val("e2", 3))])
# What a committed fast-forward of trial into main leaves on main.
AFTER_FF_EXTRA = (7, "main", "human", "command", PROMOTE, 4, [])


def build_rows(plan):
    rows = []
    hashes = {}
    prev = ZERO
    for seq, branch, pk, kind, cid, parent, outputs in plan:
        h = {
            "schema_version": 1,
            "kind": kind,
            "stream": STREAM,
            "seq": str(seq),
            "prev": prev,
            "branch": branch,
            "logical_parent": None if parent is None else hashes[parent],
            "time": {"utc": "2026-11-21T09:%02d:00Z" % seq, "mono_ns": str(1_000_000_000 * seq), "sync": "synced"},
            "principal": {"kind": pk, "commit": sha(b"identity %d" % seq)},
            "ai_assistance": "authored" if pk == "agent" else "none",
            "manifests": {"registry": bd.REGISTRY, "kernel": bd.KERNEL},
        }
        if kind == "session_header":
            h["session"] = {"session_id": "s-%04d" % seq, "app_version": "0.1.0"}
        else:
            outs = [{"digest": d, "entity": e} for e, d in outputs]
            h["command"] = {
                "id": cid, "major": 1, "class": "W2", "determinism": "D0", "seed": str(seq),
                "args_commit": sha(b"args %d" % seq), "events_commit": sha(b"events %d" % seq),
                "inputs": [], "outputs": outs, "algorithms": [],
            }
        h["hash"] = sha(cbytes(h))
        hashes[seq] = h["hash"]
        rows.append({"seq": seq, "hash": h["hash"], "stream": STREAM, "branch": branch, "kind": kind,
                     "prev_hash": prev, "header": canon(h)})
        prev = h["hash"]
    return rows, hashes


def journals():
    ff, hashes = build_rows(PLAN)
    fork, _ = build_rows(PLAN + [FORK_EXTRA])
    after, ah = build_rows(PLAN + [AFTER_FF_EXTRA])
    branches = [
        {"name": "main", "created_from": None, "protected": False},
        {"name": "trial", "created_from": hashes[2], "protected": False},
        {"name": "agent/claude/1", "created_from": hashes[2], "protected": False},
        {"name": "prot", "created_from": hashes[2], "protected": True},
        {"name": "rel", "created_from": hashes[2], "protected": True},
    ]
    return {"stream": STREAM, "branches": branches,
            "journals": {"ff": ff, "fork": fork, "after_ff": after}}, hashes


def model(js, name):
    rows = [{"seq": r["seq"], "hash": r["hash"], "stream": r["stream"], "branch": r["branch"], "kind": r["kind"],
             "prev_hash": r["prev_hash"], "header": r["header"].encode("utf-8"), "refs": []}
            for r in js["journals"][name]]
    return {"rows": rows, "branches": [{"name": b["name"], "created_from": b["created_from"]} for b in js["branches"]]}


def digest_of(v):
    return sha(cbytes(v))


# ------------------------------------------------------------------ the plan reference

class Refused(Exception):
    def __init__(self, code, reason, step=None):
        super().__init__(code + "/" + reason)
        self.code, self.reason, self.step = code, reason, step


def pre_image(p):
    return digest_of({k: p[k] for k in ("diff", "source", "source_head", "target", "target_head")})


def plan(js, case):
    op, actor, p = case["command"], case["actor"], case["params"]
    if op in (PROMOTE, PROTECTED) and actor != "human":
        raise Refused("policy", "human_only" if op == PROMOTE else "class_forbidden")
    if op == REQUEST and "note" in p and len(p["note"]) > MAX_NOTE:
        raise Refused("validation", "schema")
    table = {b["name"]: b for b in js["branches"]}
    for k in ("source", "target"):
        if p[k] not in table:
            raise Refused("not_found", "unknown_branch")
    if p["source"] == p["target"] or p["target"].startswith("agent/"):
        raise Refused("precondition", "false")
    protected = table[p["target"]]["protected"]
    if op == PROMOTE and protected:
        raise Refused("policy", "target_protected")
    if op == PROTECTED and not protected:
        raise Refused("precondition", "false")
    out = bd.outcome(model(js, case["journal"]), p["source"], p["target"])
    if "error" in out:
        raise Refused(out["error"]["code"], out["error"]["reason"])
    d = out["result"]
    if d["source"]["head"] != p["source_head"] or d["target"]["head"] != p["target_head"]:
        raise Refused("conflict", "expected_head_mismatch")
    if digest_of(d) != p["diff"]:
        raise Refused("conflict", "expected_digest_mismatch")
    if p["source_head"] == p["target_head"] or d["ancestor"] == p["source_head"]:
        raise Refused("precondition", "false")
    body = {k: p[k] for k in ("diff", "source", "source_head", "target", "target_head")}
    if op == REQUEST:
        if "note" in p:
            body["note"] = p["note"]
        return {"type": "PromotionRequested", "body": body}
    if op == PROTECTED:
        if not case["assertion_check"]:
            raise Refused("precondition", "capability_missing")
        if case["assertion_check"]["confirms"] != [pre_image(p), p["assertion"]]:
            raise Refused("policy", "no_assertion")
        body["pre_image"] = pre_image(p)
    if d["ancestor"] == p["target_head"]:
        body["mode"] = "fast_forward"
        body["entries"] = []
    else:
        steps = case["rebaser"]
        if steps is None:
            raise Refused("precondition", "capability_missing")
        if not steps:
            raise Refused("precondition", "false")
        if len(steps) > MAX_STEPS:
            raise Refused("validation", "input_limit")
        for i, s in enumerate(steps, start=1):
            if s["outcome"] not in OK_OUTCOMES:
                raise Refused("precondition", "divergent_step", step=i)
        body["mode"] = "rebased"
        body["entries"] = [{"command": s["command"], "digest": s["digest"], "outcome": s["outcome"], "step": i}
                           for i, s in enumerate(steps, start=1)]
    return {"type": "BranchPromoted" if op == PROMOTE else "ProtectedBranchPromoted", "body": body}


def plan_outcome(js, case):
    try:
        return {"event": plan(js, case)}
    except Refused as e:
        err = {"code": e.code, "reason": e.reason}
        if e.step is not None:
            err["step"] = e.step
        return {"error": err}


# ------------------------------------------------------------------ the fold reference

PROMO_KEYS = ["diff", "entries", "mode", "source", "source_head", "target", "target_head"]
REQUEST_KEYS = ["diff", "source", "source_head", "target", "target_head"]


def fold(js, log):
    table = {b["name"]: b for b in js["branches"]}
    seen = set()
    promotions = 0
    requests = []
    for entry in log:
        o, asserted, events = entry["origin"], entry["asserted"], entry["events"]
        handler = o["source"] == "handler" and o["major"] == 1
        promos = [e for e in events if e["type"] in ("BranchPromoted", "ProtectedBranchPromoted")]
        if len(promos) > 1:
            return {"error": "invariant_violated"}
        for e in events:
            t, b = e["type"], e["body"]
            if t == "PromotionRequested":
                keys = sorted(b)
                if keys not in (REQUEST_KEYS, sorted(REQUEST_KEYS + ["note"])):
                    return {"error": "invariant_violated"}
                if not (handler and o["command"] == REQUEST and o["class"] == "W1"):
                    return {"error": "invariant_violated"}
                if b["source"] not in table or b["target"] not in table or b["source"] == b["target"]:
                    return {"error": "invariant_violated"}
                if len(b.get("note", "")) > MAX_NOTE:
                    return {"error": "invariant_violated"}
                requests.append(o["kind"])
                continue
            want = PROMO_KEYS + (["pre_image"] if t == "ProtectedBranchPromoted" else [])
            if sorted(b) != sorted(want):
                return {"error": "invariant_violated"}
            if t == "BranchPromoted":
                ok = handler and o["command"] == PROMOTE and o["kind"] == "human" and o["class"] == "W2"
            else:
                ok = (handler and o["command"] == PROTECTED and o["kind"] == "human" and o["class"] == "W3"
                      and asserted and b["pre_image"] == pre_image(b))
            if not ok or o["branch"] != b["target"]:
                return {"error": "invariant_violated"}
            if b["source"] not in table or b["target"] not in table or b["source"] == b["target"]:
                return {"error": "invariant_violated"}
            if b["target"].startswith("agent/"):
                return {"error": "invariant_violated"}
            if table[b["target"]]["protected"] != (t == "ProtectedBranchPromoted"):
                return {"error": "invariant_violated"}
            ents = b["entries"]
            if b["mode"] == "fast_forward":
                if ents:
                    return {"error": "invariant_violated"}
            elif b["mode"] == "rebased":
                if not 1 <= len(ents) <= MAX_STEPS:
                    return {"error": "invariant_violated"}
                for i, en in enumerate(ents, start=1):
                    if sorted(en) != ["command", "digest", "outcome", "step"] or en["step"] != i \
                            or en["outcome"] not in OK_OUTCOMES:
                        return {"error": "invariant_violated"}
            else:
                return {"error": "invariant_violated"}
            key = (b["target"], b["target_head"], b["source_head"])
            if key in seen:
                return {"error": "invariant_violated"}
            seen.add(key)
            promotions += 1
    return {"ok": {"promotions": promotions, "requests": requests}}


# ------------------------------------------------------------------ cases

def steps(*outcomes):
    return [{"command": "feature.set_nominal@1", "digest": sha(b"re-executed %d" % i), "outcome": o}
            for i, o in enumerate(outcomes, start=1)]


def current(js, journal, source, target):
    d = bd.outcome(model(js, journal), source, target)["result"]
    return {"source": source, "source_head": d["source"]["head"], "target": target,
            "target_head": d["target"]["head"], "diff": digest_of(d)}


def plan_cases(js, hashes):
    out = []

    def add(name, journal, command, actor, params, rebaser=None, assertion_check=None):
        c = {"name": name, "journal": journal, "command": command, "actor": actor, "params": params,
             "rebaser": rebaser, "assertion_check": assertion_check}
        c["expect"] = plan_outcome(js, c)
        out.append(c)

    ff = current(js, "ff", "trial", "main")
    add("ac1_fast_forward", "ff", PROMOTE, "human", ff, rebaser=steps("IDENTICAL"))
    add("ac1_fast_forward_without_rebaser", "ff", PROMOTE, "human", ff)
    add("fast_forward_agent_branch", "ff", PROMOTE, "human", current(js, "ff", "agent/claude/1", "main"))
    fork = current(js, "fork", "trial", "main")
    add("ac2_rebased_identical", "fork", PROMOTE, "human", fork, rebaser=steps("IDENTICAL", "IDENTICAL", "IDENTICAL"))
    add("rebased_equivalent", "fork", PROMOTE, "human", fork, rebaser=steps("IDENTICAL", "EQUIVALENT"))
    add("ac3_divergent_second_step", "fork", PROMOTE, "human", fork,
        rebaser=steps("IDENTICAL", "DIVERGENT", "IDENTICAL"))
    add("failed_step", "fork", PROMOTE, "human", fork, rebaser=steps("FAILED"))
    add("rebaser_missing", "fork", PROMOTE, "human", fork)
    add("rebaser_no_steps", "fork", PROMOTE, "human", fork, rebaser=[])
    prot = current(js, "ff", "trial", "prot")
    add("ac4_target_protected", "ff", PROMOTE, "human", prot)
    pa = dict(prot, assertion="A-1")
    good = {"confirms": [pre_image(pa), "A-1"]}
    add("ac4_no_assertion", "ff", PROTECTED, "human", dict(pa, assertion="A-2"), assertion_check=good)
    add("assertion_over_another_pre_image", "ff", PROTECTED, "human", pa,
        assertion_check={"confirms": [pre_image(dict(pa, target_head=hashes[1])), "A-1"]})
    add("protected_fast_forward", "ff", PROTECTED, "human", pa, assertion_check=good)
    add("assertion_check_missing", "ff", PROTECTED, "human", pa)
    add("protected_into_unprotected", "ff", PROTECTED, "human", dict(ff, assertion="A-1"), assertion_check=good)
    rel = dict(current(js, "ff", "agent/claude/1", "rel"), assertion="A-9")
    add("protected_rebased", "ff", PROTECTED, "human", rel, rebaser=steps("IDENTICAL"),
        assertion_check={"confirms": [pre_image(rel), "A-9"]})
    add("agent_cannot_promote", "ff", PROMOTE, "agent", ff)
    add("script_cannot_promote", "ff", PROMOTE, "script", ff)
    add("agent_cannot_promote_protected", "ff", PROTECTED, "agent", pa, assertion_check=good)
    add("ac5_agent_request", "ff", REQUEST, "agent", dict(ff, note="Please adopt the e1 fix."))
    add("request_without_note", "ff", REQUEST, "human", ff)
    add("request_note_too_long", "ff", REQUEST, "agent", dict(ff, note="x" * (MAX_NOTE + 1)))
    add("request_stale_head", "ff", REQUEST, "agent", dict(ff, source_head=hashes[3]))
    add("source_moved", "ff", PROMOTE, "human", dict(ff, source_head=hashes[3]))
    add("target_moved", "fork", PROMOTE, "human", dict(fork, target_head=hashes[2]), rebaser=steps("IDENTICAL"))
    add("diff_digest_differs", "ff", PROMOTE, "human", dict(ff, diff=sha(b"another diff")))
    add("replayed_after_fast_forward", "after_ff", PROMOTE, "human", ff, rebaser=steps("IDENTICAL"))
    add("nothing_to_promote", "after_ff", PROMOTE, "human", current(js, "after_ff", "trial", "main"))
    add("unknown_source", "ff", PROMOTE, "human", dict(ff, source="nope"))
    add("unknown_target", "ff", PROMOTE, "human", dict(ff, target="nope"))
    add("same_branch", "ff", PROMOTE, "human", dict(ff, target="trial"))
    add("into_agent_branch", "ff", PROMOTE, "human", current(js, "ff", "trial", "agent/claude/1"))
    return out


def handler(command, cls, kind="human", branch="main", major=1, source="handler"):
    return {"kind": kind, "command": command, "major": major, "class": cls, "branch": branch, "source": source}


def fold_cases(js, plans):
    ev = {c["name"]: c["expect"]["event"] for c in plans if "event" in c["expect"]}
    ff, reb, prot, req = (ev["ac1_fast_forward"], ev["ac2_rebased_identical"], ev["protected_fast_forward"],
                          ev["ac5_agent_request"])
    promote = handler(PROMOTE, "W2")
    protected = handler(PROTECTED, "W3", branch="prot")
    request = handler(REQUEST, "W1", kind="agent", branch="trial")

    def e(origin, events, asserted=False):
        return {"origin": origin, "asserted": asserted, "events": events}

    def edit(event, **kw):
        x = copy.deepcopy(event)
        x["body"].update(kw)
        return x

    cases = [
        ("genuine_promotions_and_request", [e(promote, [ff]), e(promote, [reb]), e(protected, [prot], True),
                                            e(request, [req])]),
        ("replayed_promotion", [e(promote, [ff]), e(promote, [ff])]),
        ("replayed_protected_promotion", [e(protected, [prot], True), e(protected, [prot], True)]),
        ("agent_principal", [e(dict(promote, kind="agent"), [ff])]),
        ("script_principal", [e(dict(promote, kind="script"), [ff])]),
        ("bus_route_source", [e(dict(promote, source="bus_route"), [ff])]),
        ("other_command", [e(handler("journal.create_branch", "W2"), [ff])]),
        ("other_major", [e(dict(promote, major=2), [ff])]),
        ("other_class", [e(dict(promote, **{"class": "W1"}), [ff])]),
        ("entry_on_another_branch", [e(dict(promote, branch="trial"), [ff])]),
        ("unprotected_event_into_protected", [e(dict(promote, branch="prot"), [edit(ff, target="prot")])]),
        ("protected_without_assertion", [e(protected, [prot], False)]),
        ("protected_from_w2", [e(dict(protected, **{"class": "W2"}), [prot], True)]),
        ("protected_into_unprotected", [e(dict(protected, branch="main"), [edit(prot, target="main")], True)]),
        ("protected_pre_image_forged", [e(protected, [edit(prot, pre_image=sha(b"forged"))], True)]),
        ("protected_type_from_promote_branch", [e(dict(promote, branch="prot"), [prot], True)]),
        ("fast_forward_with_entries", [e(promote, [edit(ff, entries=reb["body"]["entries"])])]),
        ("rebased_without_entries", [e(promote, [edit(reb, entries=[])])]),
        ("divergent_entry", [e(promote, [edit(reb, entries=[dict(reb["body"]["entries"][0], outcome="DIVERGENT")])])]),
        ("entries_misnumbered", [e(promote, [edit(reb, entries=[dict(x, step=x["step"] + 1)
                                                                for x in reb["body"]["entries"]])])]),
        ("unknown_mode", [e(promote, [edit(ff, mode="squash")])]),
        ("two_promotions_in_one_entry", [e(promote, [ff, edit(ff, target_head=None)])]),
        ("into_agent_branch", [e(dict(promote, branch="agent/claude/1"), [edit(ff, target="agent/claude/1")])]),
        ("unknown_source_branch", [e(promote, [edit(ff, source="ghost")])]),
        ("extra_body_key", [e(promote, [edit(ff, requester="human")])]),
        ("request_from_human_script_agent", [e(dict(request, kind=k), [req]) for k in ("human", "script", "agent")]),
        ("request_from_w2", [e(dict(request, **{"class": "W2"}), [req])]),
        ("request_from_other_command", [e(dict(request, command=PROMOTE), [req])]),
        ("request_note_too_long", [e(request, [edit(req, note="x" * (MAX_NOTE + 1))])]),
    ]
    return [{"name": n, "log": log, "expect": fold(js, log)} for n, log in cases]


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def build():
    js, hashes = journals()
    plans = plan_cases(js, hashes)
    folds = fold_cases(js, plans)
    by = {c["name"]: c["expect"] for c in plans}
    fb = {c["name"]: c["expect"] for c in folds}
    names = [c["name"] for c in plans]
    assert len(names) == len(set(names))
    # AC-1: a fast-forward, no Rebaser needed.
    assert by["ac1_fast_forward"] == by["ac1_fast_forward_without_rebaser"]
    assert by["ac1_fast_forward"]["event"]["body"]["mode"] == "fast_forward"
    # AC-2: three entries, promoted from the source head.
    body = by["ac2_rebased_identical"]["event"]["body"]
    assert body["mode"] == "rebased" and len(body["entries"]) == 3 and body["source_head"] == hashes[4]
    # AC-3: nothing, the step named.
    assert by["ac3_divergent_second_step"] == {"error": {"code": "precondition", "reason": "divergent_step", "step": 2}}
    # AC-4.
    assert by["ac4_target_protected"]["error"]["reason"] == "target_protected"
    assert by["ac4_no_assertion"]["error"]["reason"] == "no_assertion"
    assert by["assertion_over_another_pre_image"]["error"]["reason"] == "no_assertion"
    # AC-5: an agent requests; it cannot promote.
    assert by["ac5_agent_request"]["event"]["type"] == "PromotionRequested"
    assert by["agent_cannot_promote"]["error"]["reason"] == "human_only"
    # Replays and moved heads are refused.
    for n in ("replayed_after_fast_forward", "source_moved", "target_moved", "request_stale_head"):
        assert by[n]["error"]["reason"] == "expected_head_mismatch", n
    assert fb["genuine_promotions_and_request"] == {"ok": {"promotions": 3, "requests": ["agent"]}}
    for n, v in fb.items():
        if n not in ("genuine_promotions_and_request", "request_from_human_script_agent"):
            assert v == {"error": "invariant_violated"}, n
    return {"journals": dump(js), "cases": dump({"plans": plans, "folds": folds})}


def main():
    files = build()
    stale = []
    for name, data in files.items():
        path = HERE / (name + ".json")
        if "--check" in sys.argv[1:]:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path.name)
        else:
            path.write_bytes(data)
            print("wrote " + path.name)
    if stale:
        print("stale fixture (run make_fixtures.py): " + ", ".join(stale))
        sys.exit(1)
    if "--check" in sys.argv[1:]:
        print("fixtures up to date")


if __name__ == "__main__":
    main()
