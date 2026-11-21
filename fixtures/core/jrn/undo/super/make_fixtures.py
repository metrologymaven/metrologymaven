#!/usr/bin/env python3
"""Generate the CORE.JRN.UNDO.SUPER fixtures with Python's standard library only.

An independent reference: nothing here is produced by running the Rust crates. Every
expected outcome below is hand-labelled and then checked against this script's own reading
of the rules (verdict(), fold(), plan() and precommit(), written from
spec/core/jrn/undo/super/NODE.md and REQ-CORE.RPL.RECON.FOLD-008, not from the Rust code).

Outputs (CC-BY-4.0 project data), next to this script:

  two_releases.json  the AC-1 journal: records R1 and R2 released on `main` by two W3 test
                     commands (`test.release_record@1`), as the per-entry records the fold
                     takes (verified-header origin, principal commitment, parameters
                     commitment, hash, promoted_from, reason_commit and its opening, outputs,
                     the logical head before the entry) and their events; plus the other logs
                     the cases use (forks, a fast-forward, a promoted copy).
  cases.json         "folds": journals (a log plus extra entries) with the outcome of folding
                     them: "ok" with, for each view head (the last entry of every branch, the
                     branch heads), exactly the supersessions and released records in that
                     head's ancestry; or "refused" at an entry index. Forged cases cover
                     FOLD-008 (a) to (e), the parameters binding and the release origin.
                     "plans": calls of journal.supersede_record@1 against a log, with the
                     event or the bus error (code, reason).
                     "precommits": the commit stage's check of one entry for one call, "ok" or
                     the bus error.

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
WHY = "wrong part revision"
INTERNAL = ("internal", "invariant_violated")
HEAD_MOVED = ("conflict", "head_moved")


def d(label):
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def jcs(v):
    """RFC 8785 bytes for the values used here (objects of strings, strings)."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def pc(old, new, reason):
    """Parameters commitment: sha256 of the canonical call parameters."""
    return "sha256:" + hashlib.sha256(jcs({"new": new, "old": old, "reason": reason})).hexdigest()


def rc(salt_hex, reason):
    """Salted reason commitment (CORE.JRN.ENTRY): sha256(salt bytes || canonical string)."""
    return "sha256:" + hashlib.sha256(bytes.fromhex(salt_hex) + jcs(reason)).hexdigest()


R1, R2, R3, R9 = d("record R1"), d("record R2"), d("record R3"), d("record R9")
H1, H2, H3, H4 = d("entry H1"), d("entry H2"), d("entry H3"), d("entry H4")
T1, T2 = d("entry T1"), d("entry T2")
S1, S2, S3 = d("entry S1"), d("entry S2"), d("entry S3")
X1, X2 = d("entry X1"), d("entry X2")
F1, F2 = d("entry F1"), d("entry F2")
M3, M4 = d("entry M3"), d("entry M4")
P1 = d("entry P1")
ALICE = d("principal alice")
SALT1, SALT2 = "11" * 16, "22" * 16
DEFAULT = object()


def entry(h, head_before, *, branch="main", kind="human", command="test.release_record", major=1, cls="W3",
          source="handler", principal=ALICE, params_commitment=DEFAULT, promoted_from=None, reason_commit=None,
          reason_opening=None, outputs=(), events=()):
    if params_commitment is DEFAULT:
        params_commitment = d("parameters of " + h)
    return {
        "hash": h,
        "branch": branch,
        "kind": kind,
        "command": command,
        "major": major,
        "class": cls,
        "source": source,
        "principal": principal,
        "params_commitment": params_commitment,
        "promoted_from": promoted_from,
        "reason_commit": reason_commit,
        "reason_opening": reason_opening,
        "outputs": list(outputs),
        "head_before": head_before,
        "events": [dict(e) for e in events],
    }


def ev(old, new, head):
    return {"type": EVENT, "body": {"head": head, "new": new, "old": old}}


def sup(h, head_before, old, new, *, head=None, why=WHY, salt=SALT1, opening=DEFAULT, **kw):
    """A supersession entry as the commit stage records it for call {old, new, why}."""
    kw.setdefault("command", COMMAND)
    kw.setdefault("params_commitment", pc(old, new, why))
    kw.setdefault("reason_commit", rc(salt, why))
    kw.setdefault("events", [ev(old, new, head_before if head is None else head)])
    kw["reason_opening"] = {"reason": why, "salt": salt} if opening is DEFAULT else opening
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
AFTER = TWO_RELEASES + [sup(S1, H2, R1, R2)]
# R1 and R2 released on a human branch `trial` too, by its own W3 entries (a root of its own).
ON_TRIAL = TWO_RELEASES + [
    entry(T1, None, branch="trial", outputs=[R1]),
    entry(T2, T1, branch="trial", outputs=[R2]),
]
# Branch `fix` forked from main after S1 superseded R1 by R2; it releases R3.
FORK_AFTER = AFTER + [entry(X1, S1, branch="fix", outputs=[R3])]
# Branch `feat` forked at H2 releases R3 and supersedes R1 by R3; main is fast-forwarded to F2
# (no entry) and then records a W2 edit M3 on top of it.
FAST_FORWARD = TWO_RELEASES + [
    entry(F1, H2, branch="feat", outputs=[R3]),
    sup(F2, F1, R1, R3, branch="feat"),
    entry(M3, F2, cls="W2", command="test.edit_record", outputs=[R9]),
]
# A promoted copy on main of a release of R3 made on another branch.
PROMOTED_RELEASE = TWO_RELEASES + [entry(P1, H2, promoted_from=d("entry on feat"), outputs=[R3])]

LOGS = {
    "two_releases": TWO_RELEASES,
    "more": MORE,
    "after_supersession": AFTER,
    "on_trial": ON_TRIAL,
    "fork_after": FORK_AFTER,
    "fast_forward": FAST_FORWARD,
    "promoted_release": PROMOTED_RELEASE,
}


# ---------------------------------------------------------------- the reference fold

def is_digest(v):
    return isinstance(v, str) and len(v) == 71 and v.startswith("sha256:") and all(
        c in "0123456789abcdef" for c in v[7:])


def ancestry(nodes, at):
    while at is not None and at in nodes:
        yield at, nodes[at]
        at = nodes[at]["parent"]


def view(nodes, at):
    """(superseded, released) in the ancestry of `at`: old -> (new, entry, reason, by); record -> first entry."""
    released, superseded = {}, {}
    for h, n in ancestry(nodes, at):
        for r in n["released"]:
            released[r] = h  # walking newest first: the last write is the first release
        if n["sup"] is not None:
            old, v = n["sup"]
            superseded.setdefault(old, v)
    return superseded, released


def verdict(nodes, e):
    """(None, node, kind) or (error, None, None); error is a (code, reason) pair."""
    if e["hash"] in nodes:
        return INTERNAL, None, None
    if e["head_before"] is not None and e["head_before"] not in nodes:
        return INTERNAL, None, None
    node = {"parent": e["head_before"], "released": [], "sup": None}
    if e["promoted_from"] is not None:
        return None, node, "promoted_copy"
    ours = [x for x in e["events"] if x["type"] == EVENT]
    if not ours:
        if e["class"] != "W3" or e["command"] == COMMAND:
            return None, node, "plain"
        rel = sorted({o for o in e["outputs"] if o != TOMBSTONE})
        if not rel:
            return None, node, "plain"
        if e["kind"] != "human" or e["source"] == "bus_route":
            return INTERNAL, None, None
        node["released"] = rel
        return None, node, "released"
    ok = (e["source"] == "handler" and e["command"] == COMMAND and e["major"] == 1 and e["class"] == "W3"
          and e["kind"] == "human")
    ok = ok and not any(x["type"].startswith("Request") for x in e["events"])
    ok = ok and len(ours) == 1 and e["params_commitment"] is not None and e["reason_commit"] is not None
    body = ours[0]["body"]
    ok = ok and isinstance(body, dict) and sorted(body) == ["head", "new", "old"] and all(
        is_digest(body[k]) for k in body)
    if ok and e["reason_opening"] is not None:
        o = e["reason_opening"]
        ok = rc(o["salt"], o["reason"]) == e["reason_commit"] and pc(
            body["old"], body["new"], o["reason"]) == e["params_commitment"]
    if not ok:
        return INTERNAL, None, None
    if e["head_before"] != body["head"]:
        return HEAD_MOVED, None, None
    old, new = body["old"], body["new"]
    superseded, released = view(nodes, e["head_before"])
    if old == new or old not in released or new not in released or old in superseded or new in superseded:
        return INTERNAL, None, None
    node["sup"] = (old, (new, e["hash"], e["reason_commit"], e["principal"]))
    return None, node, "superseded"


def fold(log):
    """('ok', nodes) or ('refused', index). Every refusal is fatal, a stale head included."""
    nodes = {}
    for i, e in enumerate(log):
        err, node, _ = verdict(nodes, e)
        if err is not None:
            return ("refused", i)
        nodes[e["hash"]] = node
    return ("ok", nodes)


def branch_heads(log):
    heads = {}
    for e in log:
        heads[e["branch"]] = e["hash"]
    return heads


def fold_expect(log):
    r = fold(log)
    if r[0] == "refused":
        return {"outcome": "refused", "entry": r[1]}
    nodes = r[1]
    views = []
    for b, at in sorted(branch_heads(log).items()):
        superseded, released = view(nodes, at)
        views.append({
            "branch": b,
            "at": at,
            "superseded": [{"old": o, "new": v[0], "entry": v[1], "reason_commit": v[2], "by": v[3]}
                           for o, v in sorted(superseded.items())],
            "released": [{"record": rec, "entry": h} for rec, h in sorted(released.items())],
        })
    return {"outcome": "ok", "views": views}


def head_of(log, branch):
    return branch_heads(log).get(branch)


def plan(log, actor, branch, params):
    """('event', body) or ('error', code, reason)."""
    if actor != "human":
        return ("error", "policy", "class_forbidden")
    n = len(params["reason"])  # Python str length counts code points, like Rust's chars().
    if n < 1 or n > MAX_REASON:
        return ("error", "validation", "schema")
    r = fold(log)
    assert r[0] == "ok"
    head = head_of(log, branch)
    superseded, released = view(r[1], head)
    old, new = params["old"], params["new"]
    if old == new or old not in released or new not in released:
        return ("error", "precondition", "not_supersedable")
    if old in superseded or new in superseded:
        return ("error", "precondition", "not_supersedable")
    if head is None:
        return ("error", "precondition", "not_supersedable")
    return ("event", {"head": head, "new": new, "old": old})


def precommit(log, e, params):
    """'ok' or (code, reason): the commit stage's check of entry `e` for call `params`."""
    r = fold(log)
    assert r[0] == "ok"
    o = e["reason_opening"]
    if o is None or o["reason"] != params["reason"]:
        return INTERNAL
    err, _, kind = verdict(r[1], e)
    if err is not None:
        return err
    return "ok" if kind == "superseded" else INTERNAL


# ---------------------------------------------------------------- hand-labelled cases

FOLDS = [
    # name, log, extra entries, hand label ("ok" or refused index), rule
    ("two_releases", "two_releases", [], "ok", "AC-1 journal: R1 and R2 released, nothing superseded"),
    ("ac1_supersede", "two_releases", [sup(S1, H2, R1, R2)], "ok",
     "AC-1: superseded_by(R1) is R2 and R1's entry stays released"),
    ("more_releases", "more", [], "ok", "a W2 output and the tombstone are not releases"),
    ("chain", "more", [sup(S1, H4, R1, R2), sup(S2, S1, R2, R3, salt=SALT2, why="second reason")], "ok",
     "R2 may itself be superseded later"),
    ("two_old_one_new", "more", [sup(S1, H4, R1, R3), sup(S2, S1, R2, R3)], "ok",
     "one new record may replace two old ones"),
    ("on_trial", "on_trial", [sup(S1, T2, R1, R2, branch="trial")], "ok",
     "(e) a supersession counts in its own branch's ancestry only"),
    # (a) origin
    ("forged_agent", "two_releases", [sup(S1, H2, R1, R2, kind="agent")], 2, "(a) the principal is human"),
    ("forged_script", "two_releases", [sup(S1, H2, R1, R2, kind="script")], 2, "(a) the principal is human"),
    ("forged_bus_route", "two_releases", [sup(S1, H2, R1, R2, source="bus_route")], 2,
     "(a) the handler added the event"),
    ("forged_class_w2", "two_releases", [sup(S1, H2, R1, R2, cls="W2")], 2, "(a) the entry is W3"),
    ("forged_major_2", "two_releases", [sup(S1, H2, R1, R2, major=2)], 2, "(a) the command is major 1"),
    # (a) parameters: the event is the call the principal made
    ("forged_event_not_the_params", "more", [sup(S1, H4, R1, R2, params_commitment=pc(R1, R3, WHY))], 4,
     "(a) the parameters commitment is that of {new, old, reason} with the event's old and new"),
    ("forged_params_other_reason", "two_releases",
     [sup(S1, H2, R1, R2, params_commitment=pc(R1, R2, "another reason"))], 2,
     "(a) the parameters commitment is that of the opened reason"),
    ("forged_reason_commit_of_another_entry", "more",
     [sup(S1, H4, R1, R2), sup(S2, S1, R2, R3, salt=SALT2, why="second reason", reason_commit=rc(SALT1, WHY))], 5,
     "(a) reason_commit opens to the entry's own reason, not another entry's"),
    ("forged_opening_salt", "two_releases",
     [sup(S1, H2, R1, R2, opening={"reason": WHY, "salt": SALT2})], 2, "(a) reason_commit opens to the opening"),
    ("reason_erased", "two_releases", [sup(S1, H2, R1, R2, opening=None)], "ok",
     "erased reason: the parameters commitment is required but cannot be recomputed"),
    ("reason_erased_unchecked", "two_releases",
     [sup(S1, H2, R1, R2, opening=None, params_commitment=pc(R1, R3, WHY))], "ok",
     "erased reason: only the presence of the commitments is checked"),
    ("no_params_commitment", "two_releases", [sup(S1, H2, R1, R2, params_commitment=None)], 2,
     "the entry records a parameters commitment"),
    ("no_params_commitment_erased", "two_releases",
     [sup(S1, H2, R1, R2, params_commitment=None, opening=None)], 2,
     "the parameters commitment is required after erasure too"),
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
    # (e) branches: ancestry, forks, fast-forward, promoted copies
    ("forged_other_branch", "two_releases",
     [entry(T1, None, branch="trial", outputs=[R3]), sup(S1, T1, R1, R2, branch="trial")], 3,
     "(e) R1 and R2 are not released in trial's ancestry"),
    ("fork_after_supersession", "fork_after", [], "ok",
     "(e) a fork created after a supersession sees it"),
    ("fork_after_supersession_again", "fork_after", [sup(X2, X1, R1, R3, branch="fix")], 4,
     "(e) R1 is already superseded in the fork's ancestry"),
    ("fork_after_supersession_other", "fork_after", [sup(X2, X1, R2, R3, branch="fix")], "ok",
     "(e) the fork may supersede the records not yet superseded"),
    ("fork_before_supersession", "two_releases",
     [entry(X1, H2, branch="fix", outputs=[R3]), sup(S1, H2, R1, R2, head=H2), sup(X2, X1, R1, R3, branch="fix")],
     "ok", "(e) a fork created before a supersession does not see it; each branch decides its own"),
    ("fast_forward", "fast_forward", [], "ok", "(e) a branch fast-forwarded over a supersession sees it"),
    ("fast_forward_then_supersede", "fast_forward", [sup(M4, M3, R2, R3)], "ok",
     "(e) after the fast-forward R3 is released at main's head"),
    ("fast_forward_again", "fast_forward", [sup(M4, M3, R1, R2)], 5,
     "(e) R1 is already superseded at main's head after the fast-forward"),
    ("promoted_copy_supersession_skipped", "two_releases",
     [sup(S1, H2, R1, R2, promoted_from=d("entry on trial"))], "ok",
     "(e) a promoted copy carrying a supersession is skipped, not refused, and supersedes nothing"),
    ("promoted_copy_release_skipped", "promoted_release", [], "ok",
     "(e) a promoted copy releases nothing on the target"),
    ("promoted_copy_release_not_released", "promoted_release", [sup(S1, P1, R1, R3)], 3,
     "(e) R3 from a promoted copy is not a released record"),
    ("promoted_copy_by_agent_skipped", "two_releases",
     [entry(P1, H2, kind="agent", promoted_from=d("entry on feat"), outputs=[R3])], "ok",
     "(e) promoted copies are skipped before the release origin check"),
    # release origin: a release is a human's W3 command
    ("release_by_agent", "two_releases", [entry(H3, H2, kind="agent", outputs=[R3])], 2,
     "a W3 release by an agent is refused"),
    ("release_by_script", "two_releases", [entry(H3, H2, kind="script", outputs=[R3])], 2,
     "a W3 release by a script is refused"),
    ("release_by_bus_route", "two_releases", [entry(H3, H2, source="bus_route", outputs=[R3])], 2,
     "a W3 release whose events the routing module added is refused"),
    ("release_by_bus_release", "two_releases", [entry(H3, H2, source="bus_release", outputs=[R3])], "ok",
     "a human W3 entry the commit stage added release protection to is a release"),
    ("agent_w3_tombstone_only", "two_releases", [entry(H3, H2, kind="agent", outputs=[TOMBSTONE])], "ok",
     "a W3 entry with only the tombstone releases nothing"),
    ("agent_w2_output", "two_releases", [entry(H3, H2, kind="agent", cls="W2", outputs=[R3])], "ok",
     "a W2 output is not a release, whoever made it"),
    # the journal itself
    ("unknown_parent", "two_releases", [entry(H3, d("nowhere"), outputs=[R3])], 2,
     "every logical parent is an entry folded before"),
    ("folded_twice", "two_releases", [entry(H1, H2, outputs=[R3])], 2, "an entry is folded once"),
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
    ("no_reason_commit", "two_releases", [sup(S1, H2, R1, R2, reason_commit=None, opening=None)], 2,
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


def params(old, new, why=WHY):
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
     "records are looked up at the call's branch head"),
    ("on_trial", "on_trial", "human", "trial", params(R1, R2), {"head": T2, "new": R2, "old": R1},
     "a supersession on a human branch with its own releases"),
    ("chain_after", "after_supersession", "human", "main", params(R2, R1), ("precondition", "not_supersedable"),
     "R1 is superseded; it cannot replace R2"),
    ("fork_after_superseded", "fork_after", "human", "fix", params(R1, R3), ("precondition", "not_supersedable"),
     "a fork created after a supersession sees it"),
    ("fork_after_other", "fork_after", "human", "fix", params(R2, R3), {"head": X1, "new": R3, "old": R2},
     "the fork sees main's releases before the fork and its own"),
    ("fast_forward_superseded", "fast_forward", "human", "main", params(R1, R2),
     ("precondition", "not_supersedable"), "main fast-forwarded over F2 sees R1 superseded"),
    ("fast_forward_release", "fast_forward", "human", "main", params(R2, R3), {"head": M3, "new": R3, "old": R2},
     "main fast-forwarded over F1 sees R3 released"),
    ("promoted_copy_release", "promoted_release", "human", "main", params(R1, R3),
     ("precondition", "not_supersedable"), "a promoted copy is not a release"),
]

PRECOMMITS = [
    # name, log, entry, call params, hand label ("ok" or (code, reason)), rule
    ("ok", "two_releases", sup(S1, H2, R1, R2), params(R1, R2), "ok", "the entry of the call it was planned for"),
    ("event_not_the_params", "more", sup(S1, H4, R1, R2, params_commitment=pc(R1, R3, WHY)), params(R1, R3),
     INTERNAL, "the event disagrees with the call's parameters"),
    ("reason_commit_of_another_entry", "two_releases",
     sup(S1, H2, R1, R2, reason_commit=rc(SALT2, "another reason")), params(R1, R2), INTERNAL,
     "reason_commit belongs to another entry"),
    ("reason_not_the_calls", "two_releases", sup(S1, H2, R1, R2), params(R1, R2, "another reason"), INTERNAL,
     "the entry's reason is not the call's"),
    ("no_opening", "two_releases", sup(S1, H2, R1, R2, opening=None), params(R1, R2), INTERNAL,
     "the commit stage holds the opening"),
    ("head_moved", "more", sup(S1, H4, R1, R2, head=H2), params(R1, R2), HEAD_MOVED,
     "(d) the head moved since the call was planned"),
    ("concurrent_pair", "after_supersession", sup(S2, S1, R1, R2, head=H2), params(R1, R2), HEAD_MOVED,
     "(d) a second call decided against the same head"),
    ("agent", "two_releases", sup(S1, H2, R1, R2, kind="agent"), params(R1, R2), INTERNAL,
     "(a) the principal is human"),
    ("not_a_supersession", "two_releases", entry(S1, H2, command=COMMAND, params_commitment=pc(R1, R2, WHY),
                                                  reason_commit=rc(SALT1, WHY),
                                                  reason_opening={"reason": WHY, "salt": SALT1}),
     params(R1, R2), INTERNAL, "the call's entry holds its RecordSuperseded"),
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
    pres = []
    names = set()
    for name, log, e, p, label, rule in PRECOMMITS:
        assert name not in names, name
        names.add(name)
        got = precommit(LOGS[log], e, p)
        assert got == label, f"{name}: hand label {label}, precommit {got}"
        pres.append({"name": name, "rule": rule, "log": log, "entry": e, "params": p,
                     "expect": "ok" if got == "ok" else list(got)})
    return {"folds": folds, "plans": plans, "precommits": pres}


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
