#!/usr/bin/env python3
"""Generate the CORE.JRN.UNDO.SUPER fixtures with Python's standard library only.

An independent reference: nothing here is produced by running the Rust crates. Every
expected outcome below is hand-labelled and then checked against this script's own reading
of the rules (fold() and plan(), written from spec/core/jrn/undo/super/NODE.md and
REQ-CORE.RPL.RECON.FOLD-008, not from the Rust code).

Outputs (CC-BY-4.0 project data), next to this script:

  two_releases.json  the AC-1 journal: records R1 and R2 released on `main` by two W3 test
                     commands (`test.release_record@1`), as the per-entry records the fold
                     takes (verified-header origin, hash, promoted_from, reason_commit,
                     outputs, the logical head before the entry) and their events; plus the
                     other logs the cases use.
  cases.json         "folds": journals (a log plus extra entries) with the outcome of folding
                     them: "ok" with every supersession and released record, or "refused" at
                     an entry index. Forged cases cover FOLD-008 (a) to (e).
                     "plans": calls of journal.supersede_record@1 against a log, with the
                     event or the bus error (code, reason).

Run:   python fixtures/core/jrn/undo/super/make_fixtures.py
Check: python fixtures/core/jrn/undo/super/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

COMMAND = "journal.supersede_record"
EVENT = "RecordSuperseded"
MAX_REASON = 2000
TOMBSTONE = "sha256:" + "0" * 64


def d(label):
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


R1, R2, R3, R9 = d("record R1"), d("record R2"), d("record R3"), d("record R9")
H1, H2, H3, H4 = d("entry H1"), d("entry H2"), d("entry H3"), d("entry H4")
T1 = d("entry T1")
RC = d("reason commitment 1")


def entry(h, head_before, *, branch="main", kind="human", command="test.release_record", major=1, cls="W3",
          source="handler", promoted_from=None, reason_commit=None, outputs=(), events=()):
    return {
        "hash": h,
        "branch": branch,
        "kind": kind,
        "command": command,
        "major": major,
        "class": cls,
        "source": source,
        "promoted_from": promoted_from,
        "reason_commit": reason_commit,
        "outputs": list(outputs),
        "head_before": head_before,
        "events": [dict(e) for e in events],
    }


def ev(old, new, head):
    return {"type": EVENT, "body": {"head": head, "new": new, "old": old}}


def sup(h, head_before, old, new, *, head=None, **kw):
    kw.setdefault("command", COMMAND)
    kw.setdefault("reason_commit", RC)
    kw.setdefault("events", [ev(old, new, head_before if head is None else head)])
    return entry(h, head_before, **kw)


TWO_RELEASES = [
    entry(H1, None, outputs=[R1]),
    entry(H2, H1, outputs=[R2]),
]
# Two releases, a W2 output that is not a release, and a W3 entry releasing R3 and a deletion.
MORE = TWO_RELEASES + [
    entry(H3, H2, cls="W2", command="test.edit_record", outputs=[R9]),
    entry(H4, H3, outputs=[R3, TOMBSTONE]),
]
S1 = d("entry S1")
AFTER = TWO_RELEASES + [sup(S1, H2, R1, R2)]
# R1 and R2 released on a human branch `trial` too, by its own W3 entries.
ON_TRIAL = TWO_RELEASES + [
    entry(T1, None, branch="trial", outputs=[R1]),
    entry(d("entry T2"), T1, branch="trial", outputs=[R2]),
]

LOGS = {"two_releases": TWO_RELEASES, "more": MORE, "after_supersession": AFTER, "on_trial": ON_TRIAL}


# ---------------------------------------------------------------- the reference fold

def is_digest(v):
    return isinstance(v, str) and len(v) == 71 and v.startswith("sha256:") and all(
        c in "0123456789abcdef" for c in v[7:])


def fold(log):
    """('ok', superseded, released) or ('refused', index)."""
    released = {}  # (branch, record) -> entry
    superseded = {}  # (branch, old) -> (new, entry, reason_commit)
    for i, e in enumerate(log):
        ours = [x for x in e["events"] if x["type"] == EVENT]
        b = e["branch"]
        if not ours:
            if e["class"] == "W3" and e["command"] != COMMAND:
                for o in e["outputs"]:
                    if o != TOMBSTONE:
                        released.setdefault((b, o), e["hash"])
            continue
        ok = (e["source"] == "handler" and e["command"] == COMMAND and e["major"] == 1 and e["class"] == "W3"
              and e["kind"] == "human")
        ok = ok and not any(x["type"].startswith("Request") for x in e["events"])
        ok = ok and len(ours) == 1 and e["promoted_from"] is None and e["reason_commit"] is not None
        body = ours[0]["body"]
        ok = ok and isinstance(body, dict) and sorted(body) == ["head", "new", "old"] and all(
            is_digest(body[k]) for k in body)
        ok = ok and e["head_before"] == body["head"]
        if ok:
            old, new = body["old"], body["new"]
            ok = (old != new and (b, old) in released and (b, new) in released and (b, old) not in superseded
                  and (b, new) not in superseded)
        if not ok:
            return ("refused", i)
        superseded[(b, body["old"])] = (body["new"], e["hash"], e["reason_commit"])
    return ("ok", superseded, released)


def fold_expect(log):
    r = fold(log)
    if r[0] == "refused":
        return {"outcome": "refused", "entry": r[1]}
    _, superseded, released = r
    return {
        "outcome": "ok",
        "superseded": [{"branch": b, "old": o, "new": v[0], "entry": v[1], "reason_commit": v[2]}
                       for (b, o), v in sorted(superseded.items())],
        "released": [{"branch": b, "record": r, "entry": h} for (b, r), h in sorted(released.items())],
    }


def head_of(log, branch):
    on = [e["hash"] for e in log if e["branch"] == branch]
    return on[-1] if on else None


def plan(log, actor, branch, params):
    """('event', body) or ('error', code, reason)."""
    if actor != "human":
        return ("error", "policy", "class_forbidden")
    n = len(params["reason"])  # Python str length counts code points, like Rust's chars().
    if n < 1 or n > MAX_REASON:
        return ("error", "validation", "schema")
    r = fold(log)
    assert r[0] == "ok"
    _, superseded, released = r
    old, new = params["old"], params["new"]
    if old == new or (branch, old) not in released or (branch, new) not in released:
        return ("error", "precondition", "not_supersedable")
    if (branch, old) in superseded or (branch, new) in superseded:
        return ("error", "precondition", "not_supersedable")
    head = head_of(log, branch)
    if head is None:
        return ("error", "precondition", "not_supersedable")
    return ("event", {"head": head, "new": new, "old": old})


# ---------------------------------------------------------------- hand-labelled cases

S2, S3 = d("entry S2"), d("entry S3")

FOLDS = [
    # name, log, extra entries, hand label ("ok" or refused index), rule
    ("two_releases", "two_releases", [], "ok", "AC-1 journal: R1 and R2 released, nothing superseded"),
    ("ac1_supersede", "two_releases", [sup(S1, H2, R1, R2)], "ok",
     "AC-1: superseded_by(R1) is R2 and R1's entry stays released"),
    ("more_releases", "more", [], "ok", "a W2 output and the tombstone are not releases"),
    ("chain", "more", [sup(S1, H4, R1, R2), sup(S2, S1, R2, R3)], "ok", "R2 may itself be superseded later"),
    ("two_old_one_new", "more", [sup(S1, H4, R1, R3), sup(S2, S1, R2, R3)], "ok",
     "one new record may replace two old ones"),
    ("on_trial", "on_trial", [sup(S1, d("entry T2"), R1, R2, branch="trial")], "ok",
     "(e) a supersession counts on its own branch"),
    # (a) origin
    ("forged_agent", "two_releases", [sup(S1, H2, R1, R2, kind="agent")], 2, "(a) the principal is human"),
    ("forged_script", "two_releases", [sup(S1, H2, R1, R2, kind="script")], 2, "(a) the principal is human"),
    ("forged_bus_route", "two_releases", [sup(S1, H2, R1, R2, source="bus_route")], 2,
     "(a) the handler added the event"),
    ("forged_class_w2", "two_releases", [sup(S1, H2, R1, R2, cls="W2")], 2, "(a) the entry is W3"),
    ("forged_major_2", "two_releases", [sup(S1, H2, R1, R2, major=2)], 2, "(a) the command is major 1"),
    # (b) reserved effect
    ("forged_other_command", "two_releases", [sup(S1, H2, R1, R2, command="test.release_record")], 2,
     "(b) RecordSuperseded only from journal.supersede_record@1"),
    # (c) requests
    ("forged_with_request_filed", "two_releases",
     [sup(S1, H2, R1, R2, events=[ev(R1, R2, H2), {"type": "RequestFiled", "body": {}}])], 2,
     "(c) no Request* event next to it"),
    ("forged_with_request_approved", "two_releases",
     [sup(S1, H2, R1, R2, events=[{"type": "RequestApproved", "body": {}}, ev(R1, R2, H2)])], 2,
     "(c) an approved request does not carry a supersession"),
    # (d) decided-against state
    ("forged_stale_head", "more", [sup(S1, H4, R1, R2, head=H2)], 4, "(d) head is the branch head before the entry"),
    ("forged_made_up_head", "two_releases", [sup(S1, H2, R1, R2, head=d("nowhere"))], 2,
     "(d) head is the branch head before the entry"),
    ("replayed_event", "more", [sup(S1, H4, R1, R2), sup(S2, S1, R1, R2, head=H4)], 5,
     "(d) the same event replayed later is refused"),
    ("concurrent_pair", "more", [sup(S1, H4, R1, R2), sup(S2, S1, R1, R3, head=H4)], 5,
     "(d) two supersessions decided against the same head"),
    # (e) branches
    ("forged_other_branch", "two_releases", [sup(S1, T1, R1, R2, branch="trial")], 2,
     "(e) R1 and R2 are not released on trial"),
    ("forged_promoted_copy", "two_releases", [sup(S1, H2, R1, R2, promoted_from=d("entry on trial"))], 2,
     "(e) a promoted copy never carries a supersession"),
    # body and header
    ("two_events", "two_releases", [sup(S1, H2, R1, R2, events=[ev(R1, R2, H2), ev(R1, R2, H2)])], 2,
     "one RecordSuperseded per entry"),
    ("extra_key", "two_releases",
     [sup(S1, H2, R1, R2, events=[{"type": EVENT, "body": {"head": H2, "new": R2, "old": R1, "reason": "x"}}])], 2,
     "exactly the keys head, new, old"),
    ("missing_key", "two_releases", [sup(S1, H2, R1, R2, events=[{"type": EVENT, "body": {"head": H2, "old": R1}}])],
     2, "exactly the keys head, new, old"),
    ("not_a_digest", "two_releases",
     [sup(S1, H2, R1, R2, events=[{"type": EVENT, "body": {"head": H2, "new": "R2", "old": R1}}])], 2,
     "every value a digest"),
    ("uppercase_digest", "two_releases",
     [sup(S1, H2, R1, R2, events=[{"type": EVENT, "body": {"head": H2, "new": R2.upper().replace("SHA256", "sha256"),
                                                           "old": R1}}])], 2, "lowercase digests"),
    ("body_not_object", "two_releases", [sup(S1, H2, R1, R2, events=[{"type": EVENT, "body": [R1, R2]}])], 2,
     "the body is an object"),
    ("no_reason_commit", "two_releases", [sup(S1, H2, R1, R2, reason_commit=None)], 2,
     "the header records a reason commitment"),
    ("self_supersession", "two_releases", [sup(S1, H2, R1, R1)], 2, "old and new differ"),
    ("old_not_released", "more", [sup(S1, H4, R9, R2)], 4, "a W2 output is not a released record"),
    ("new_not_released", "two_releases", [sup(S1, H2, R1, d("record nowhere"))], 2, "new is a released record"),
    ("tombstone_not_released", "more", [sup(S1, H4, R1, TOMBSTONE)], 4, "the tombstone is not a record"),
    ("old_already_superseded", "after_supersession", [sup(S2, S1, R1, R2, head=S1)], 3, "old not yet superseded"),
    ("new_already_superseded", "more", [sup(S1, H4, R1, R2), sup(S2, S1, R3, R1)], 5,
     "new must not be superseded"),
    ("supersede_outputs_are_not_releases", "two_releases",
     [sup(S1, H2, R1, R2, outputs=[R3]), sup(S2, S1, R2, R3)], 3,
     "a supersession entry's outputs release nothing"),
    ("refusal_is_fatal", "two_releases", [sup(S1, H2, R1, R2, kind="agent"), sup(S2, S1, R1, R2, head=S1)], 2,
     "(f) the fold stops at the first refused entry"),
]


def reason(n, ch="x"):
    return ch * n


def params(old, new, why="wrong part revision"):
    return {"new": new, "old": old, "reason": why}


PLANS = [
    # name, log, actor, branch, params, hand label (event body or (code, reason)), rule
    ("ac1_human", "two_releases", "human", "main", params(R1, R2), {"head": H2, "new": R2, "old": R1},
     "AC-1: a human supersedes R1 by R2"),
    ("agent", "two_releases", "agent", "main", params(R1, R2), ("policy", "class_forbidden"), "AC-2: W3 is human only"),
    ("script", "two_releases", "script", "main", params(R1, R2), ("policy", "class_forbidden"), "W3 is human only"),
    ("agent_before_reason", "two_releases", "agent", "main", params(R1, R2, ""), ("policy", "class_forbidden"),
     "the caller is checked first"),
    ("reason_empty", "two_releases", "human", "main", params(R1, R2, ""), ("validation", "schema"), "reason 1..2000"),
    ("reason_2000", "two_releases", "human", "main", params(R1, R2, reason(2000)), {"head": H2, "new": R2, "old": R1},
     "reason 1..2000 characters"),
    ("reason_2000_multibyte", "two_releases", "human", "main", params(R1, R2, reason(2000, "é")),
     {"head": H2, "new": R2, "old": R1}, "characters, not bytes"),
    ("reason_2001", "two_releases", "human", "main", params(R1, R2, reason(2001)), ("validation", "schema"),
     "reason 1..2000"),
    ("reason_before_pair", "two_releases", "human", "main", params(R1, R1, ""), ("validation", "schema"),
     "the reason is checked before the records"),
    ("self", "two_releases", "human", "main", params(R1, R1), ("precondition", "not_supersedable"),
     "AC-3: old and new differ"),
    ("old_not_released", "more", "human", "main", params(R9, R2), ("precondition", "not_supersedable"),
     "AC-3: old is not a released record"),
    ("new_not_released", "two_releases", "human", "main", params(R1, d("record nowhere")),
     ("precondition", "not_supersedable"), "new is a released record"),
    ("old_already_superseded", "after_supersession", "human", "main", params(R1, R2),
     ("precondition", "not_supersedable"), "AC-3: old already superseded"),
    ("new_already_superseded", "after_supersession", "human", "main", params(R2, R1),
     ("precondition", "not_supersedable"), "new must not be superseded"),
    ("other_branch", "two_releases", "human", "trial", params(R1, R2), ("precondition", "not_supersedable"),
     "records are looked up on the call's branch"),
    ("on_trial", "on_trial", "human", "trial", params(R1, R2), {"head": d("entry T2"), "new": R2, "old": R1},
     "a supersession on a human branch with its own releases"),
    ("chain_after", "after_supersession", "human", "main", params(R2, R1), ("precondition", "not_supersedable"),
     "R1 is superseded; it cannot replace R2"),
]


def cases():
    folds = []
    names = set()
    for name, log, extra, label, rule in FOLDS:
        assert name not in names, name
        names.add(name)
        full = LOGS[log] + extra
        got = fold_expect(full)
        want_ok = label == "ok"
        assert (got["outcome"] == "ok") == want_ok, f"{name}: hand label {label}, fold {got}"
        if not want_ok:
            assert got["entry"] == label, f"{name}: hand label {label}, fold {got}"
        folds.append({"name": name, "rule": rule, "entries": full, "expect": got})
    plans = []
    names = set()
    for name, log, actor, branch, p, label, rule in PLANS:
        assert name not in names, name
        names.add(name)
        got = plan(LOGS[log], actor, branch, p)
        if isinstance(label, dict):
            assert got == ("event", label), f"{name}: hand label {label}, plan {got}"
            expect = {"event": {"type": EVENT, "body": label}}
        else:
            assert got == ("error",) + label, f"{name}: hand label {label}, plan {got}"
            expect = {"error": list(label)}
        plans.append({"name": name, "rule": rule, "log": log, "actor": actor, "branch": branch,
                      "head": head_of(LOGS[log], branch), "params": p, "expect": expect})
    return {"folds": folds, "plans": plans}


def dump(data):
    return (json.dumps(data, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] not in ([], ["--check"]):
        print("usage: make_fixtures.py [--check]")
        return 2
    out = {
        HERE / "two_releases.json": dump({"logs": LOGS, "records": {"R1": R1, "R2": R2, "R3": R3, "R9": R9}}),
        HERE / "cases.json": dump(cases()),
    }
    if check:
        stale = [p.name for p, b in out.items() if not p.exists() or p.read_bytes() != b]
        if stale:
            print("stale fixtures (run make_fixtures.py):", ", ".join(stale))
            return 1
        print("fixtures up to date")
        return 0
    for p, b in out.items():
        p.write_bytes(b)
    print("wrote", ", ".join(sorted(p.name for p in out)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
