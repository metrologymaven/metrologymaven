#!/usr/bin/env python3
"""Independent reference for the DMT row review state machine (GDT.RES.ROWFLOW.STATE).

Stdlib only. It writes the synthetic rows, the synthetic roster and one case file per
acceptance criterion, and labels every event of every case with the outcome the spec's rules
give (REQ-GDT.RES.ROWFLOW.STATE-001 .. 008, REQ-GDT.RES.ROWFLOW-006, REQ-GDT.RES.STATUS-001).
The Rust crate is never run to produce an expected value.

    python make_state_cases.py           # rewrite the fixtures
    python make_state_cases.py --check   # verify the committed fixtures are current

Everything here is invented: keys, designators, editions, clause numbers, reviewer IDs and
principal identities. No standards text. CC0.

Rules this reference applies (the pinned check order of a review event):

 1. revision state `signed_off`            -> bad_transition{state, event}
 2. author not a human principal           -> agent_authored_revision
 3. reviewer not a human principal         -> reviewer_not_human
 4. date before the proposal or last review -> backdated_review
 5. reviewed digest != content digest      -> stale_review
 6. reviewer identity == author identity   -> reviewer_is_author   (ASCII case-insensitive)
 7. reviewer holds no role for the dialect -> reviewer_not_in_roster
 8. reviewer's roster ID == author's       -> reviewer_is_author
 9. approve while holding a live approval  -> duplicate_reviewer
10. attestations                           -> bad_attestation{placeholder_not_allowed} /
                                              attestation_missing{designator, edition}
11. IDR (request_changes only)             -> bad_idr{reason} / field_too_long{position}

A proposal starts a new revision: number = previous + 1, state draft, no reviews,
derived_from = the previous revision's (number, content digest). A row whose own `review`
field is not draft is review_field_not_draft (checked first); a different key is
key_mismatch. The content digest is SHA-256 over RFC 8785 canonical JSON of the row without
`review` (for these ASCII, number-free documents, json.dumps with sorted keys and no spaces).
"""

import copy
import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
DAY = "2026-11-20"
MAX_POSITION_CHARS = 280
MAX_POSITIONS = 8


def canon(v):
    s = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert s.isascii(), "reference only covers ASCII documents"
    return s


def content_digest(row):
    content = {k: v for k, v in row.items() if k != "review"}
    return "sha256:" + hashlib.sha256(canon(content).encode("utf-8")).hexdigest()


# ------------------------------------------------------------------ synthetic rows

def chain(variant, steps):
    return {"variant": variant, "steps": steps}


STEPS = [
    {"kind": "partition", "alg": "syn.partition.whole_feature@0.1.0", "params": {}},
    {"kind": "extraction", "alg": "syn.extract.all_points@0.1.0", "params": {"min_points": "4"}},
    {"kind": "association", "alg": "syn.assoc.min_zone@0.1.0", "params": {"constrained": True}},
    {"kind": "evaluation", "alg": "syn.eval.max_deviation@0.1.0", "params": {}},
]

SUPPORTED = {
    "key": "dmt:asme/flatness/none@1",
    "dialect": "asme",
    "editions": ["syn-a-2001", "syn-b-2010"],
    "characteristic": "flatness",
    "modifiers": [],
    "clauses": [
        {"designator": "syn-std-a", "edition": "2001", "clause": "4.2"},
        {"designator": "syn-std-b", "edition": "2010", "clause": "7"},
    ],
    "variants": [chain("surface", STEPS)],
    "default_variant": "surface",
    "actual_value": "av.synthetic.flatness@1",
    "decision_rule_hint": "dr.synthetic.default@1",
    "ors_cases": ["ors:synthetic/flatness.001"],
    "status": "supported",
    "limitations": [],
    "review": {"state": "draft"},
    "notes": "Synthetic row for workflow tests; not an interpretation of any standard.",
    "synthetic": True,
}


def variant_of(base, **changes):
    row = copy.deepcopy(base)
    row.update(changes)
    return row


PLACEHOLDER = variant_of(
    SUPPORTED,
    key="dmt:iso/flatness/none@1",
    dialect="iso",
    editions=["syn-c-2015"],
    clauses=[{"designator": "syn-std-c", "edition": "2015", "clause": "5.1"}],
    variants=[chain("placeholder", [])],
    default_variant="placeholder",
    status="unsupported",
    notes="Synthetic placeholder row; no chain yet.",
)

ROWS = {
    "supported": SUPPORTED,
    "supported_v2": variant_of(SUPPORTED, notes="Synthetic row, second wording of the note."),
    "supported_review_only": variant_of(SUPPORTED, review={"state": "signed_off", "reviewers": ["r1", "r2"]}),
    "supported_synthetic_flip": variant_of(SUPPORTED, synthetic=False),
    "placeholder": PLACEHOLDER,
    "other_key": variant_of(SUPPORTED, key="dmt:asme/circularity/none@1", characteristic="circularity"),
}

# ------------------------------------------------------------------ synthetic roster

ROSTER = {
    "version": "dmt-reviewers/v1",
    "entries": [
        {"id": "r1", "display_name": "Reviewer One (fictional)", "principals": ["human:rev-one"],
         "dialects": ["asme", "iso"], "basis": "community", "valid_from": "2026-01-01"},
        {"id": "r2", "display_name": "Reviewer Two (fictional)", "principals": ["human:rev-two"],
         "dialects": ["asme", "iso"], "basis": "partner", "valid_from": "2026-01-01"},
        {"id": "r3", "display_name": "Reviewer Three (fictional)", "principals": ["human:rev-three"],
         "dialects": ["iso"], "basis": "community", "valid_from": "2026-01-01"},
        {"id": "r4", "display_name": "Author Four (fictional)", "principals": ["human:author-four", "human:author-four-alt"],
         "dialects": ["asme"], "basis": "paid_author", "valid_from": "2026-01-01"},
        {"id": "r5", "display_name": "Reviewer Five (fictional)", "principals": ["human:rev-five", "human:rev-five-alt"],
         "dialects": ["asme"], "basis": "community", "valid_from": "2026-01-01", "valid_until": "2026-11-01"},
    ],
}


def has_role(principal, dialect, at):
    """The reviewer ID `principal` (a roster identity) holds for `dialect` on `at`."""
    for e in ROSTER["entries"]:
        if principal in e["principals"] and dialect in e["dialects"]:
            if e["valid_from"] <= at and ("valid_until" not in e or at < e["valid_until"]):
                return e["id"]
    return None


def roster_identity(p):
    return "human:" + p["id"].lower() if p["kind"] == "human" else None


# ------------------------------------------------------------------ the reference machine

class Flow(Exception):
    def __init__(self, code, **detail):
        super().__init__(code)
        self.code = code
        self.detail = detail


def propose(prev, row_name, author, at):
    row = ROWS[row_name]
    if row["review"] != {"state": "draft"}:
        raise Flow("review_field_not_draft")
    digest = content_digest(row)
    if prev is not None and prev["key"] != row["key"]:
        raise Flow("key_mismatch")
    human = author["kind"] == "human"
    return {
        "key": row["key"],
        "row": row_name,
        "revision": 1 if prev is None else prev["revision"] + 1,
        "content_digest": digest,
        "author": author,
        "author_kind": author["kind"],
        "provenance": "human" if human else "agent",
        "derived_from": None if prev is None else {"revision": prev["revision"], "content_digest": prev["content_digest"]},
        "state": "draft",
        "proposed_at": at,
        "reviews": [],
        "idrs": [],
    }


def live_approvals(rev):
    out = []
    for r in rev["reviews"]:
        if r["decision"] == "request_changes":
            out = []
        else:
            out.append(r["reviewer"])
    return out


def cited(row):
    seen = []
    for c in row["clauses"]:
        pair = (c["designator"], c["edition"])
        if pair not in seen:
            seen.append(pair)
    return sorted(seen)


def is_placeholder(row):
    return row["status"] == "unsupported" and all(len(v["steps"]) == 0 for v in row["variants"])


def review(rev, ev):
    row = ROWS[rev["row"]]
    decision = ev["decision"]
    if rev["state"] == "signed_off":
        raise Flow("bad_transition", state="signed_off", event=decision)
    if rev["author_kind"] != "human":
        raise Flow("agent_authored_revision")
    who = ev["reviewer"]
    if who["kind"] != "human":
        raise Flow("reviewer_not_human")
    if ev["at"] < rev["proposed_at"] or (rev["reviews"] and ev["at"] < rev["reviews"][-1]["at"]):
        raise Flow("backdated_review")
    reviewed = rev["content_digest"] if ev["reviewed"] == "current" else content_digest(ROWS[ev["reviewed"]])
    if reviewed != rev["content_digest"]:
        raise Flow("stale_review")
    if who["id"].lower() == rev["author"]["id"].lower():
        raise Flow("reviewer_is_author")
    rid = has_role(roster_identity(who), row["dialect"], ev["at"])
    if rid is None:
        raise Flow("reviewer_not_in_roster")
    author_identity = roster_identity(rev["author"])
    if author_identity is not None and has_role(author_identity, row["dialect"], ev["at"]) == rid:
        raise Flow("reviewer_is_author")
    if decision == "approve" and rid in live_approvals(rev):
        raise Flow("duplicate_reviewer")
    attest = ev["attest"]
    if attest == "placeholder" and not is_placeholder(row):
        raise Flow("bad_attestation", reason="placeholder_not_allowed")
    placeholder_ok = attest == "placeholder" and is_placeholder(row)
    if not placeholder_ok:
        held = set() if attest in ("none", "placeholder") else {tuple(p) for p in attest}
        if attest == "cited":
            held = set(cited(row))
        for d, e in cited(row):
            if (d, e) not in held:
                raise Flow("attestation_missing", designator=d, edition=e)
    idr = ev.get("idr")
    new = copy.deepcopy(rev)
    if idr is not None:
        code = idr["question_code"]
        if not (1 <= len(code) <= 64 and code[0].isascii() and code[0].islower()
                and all(c.isascii() and (c.islower() or c.isdigit() or c == "_") for c in code)):
            raise Flow("bad_idr", reason="bad_question_code")
        positions = idr["positions"]
        if not 1 <= len(positions) <= MAX_POSITIONS:
            raise Flow("bad_idr", reason="bad_position_count")
        for p in positions:
            if len(p) > MAX_POSITION_CHARS:
                raise Flow("field_too_long", field="position")
            if p == "" or any(ord(c) < 0x20 or 0x7f <= ord(c) < 0xa0 for c in p):
                raise Flow("bad_idr", reason="bad_position")
        new["idrs"].append({"id": "idr-%d-%d" % (rev["revision"], len(rev["idrs"]) + 1), "positions": len(positions)})
    new["reviews"].append({"reviewer": rid, "decision": decision, "at": ev["at"]})
    if decision == "request_changes":
        new["state"] = "draft"
    else:
        new["state"] = "signed_off" if len(set(live_approvals(new))) >= 2 else "under_review"
    return new


def summary(rev):
    approvals = sorted(set(live_approvals(rev)))
    if rev["state"] == "signed_off":
        mapped = {"state": "signed_off", "reviewers": approvals}
    else:
        mapped = {"state": rev["state"]}
    return {
        "revision": rev["revision"],
        "key": rev["key"],
        "state": rev["state"],
        "content_digest": rev["content_digest"],
        "author_kind": rev["author_kind"],
        "provenance": rev["provenance"],
        "derived_from": rev["derived_from"],
        "approvals": approvals,
        "reviews": len(rev["reviews"]),
        "review_state": mapped,
        "idrs": [{"id": i["id"], "status": "open", "positions": i["positions"]} for i in rev["idrs"]],
    }


def run(events):
    rev = None
    out = []
    for ev in events:
        try:
            if ev["op"] == "propose":
                rev = propose(rev, ev["row"], ev["author"], ev["at"])
            else:
                rev = review(rev, ev)
            out.append({"ok": summary(rev)})
        except Flow as f:
            out.append({"error": {"code": f.code, "detail": f.detail}})
    return out


# ------------------------------------------------------------------ cases

AUTHOR = {"kind": "human", "id": "author-a"}
AGENT = {"kind": "agent", "id": "agent-client"}
R1 = {"kind": "human", "id": "rev-one"}
R2 = {"kind": "human", "id": "rev-two"}


def prop(row="supported", author=AUTHOR):
    return {"op": "propose", "row": row, "author": author, "at": DAY}


def rv(who, decision="approve", attest="cited", reviewed="current", at=DAY, idr=None):
    ev = {"op": "review", "reviewer": who, "decision": decision, "attest": attest, "reviewed": reviewed, "at": at}
    if idr is not None:
        ev["idr"] = idr
    return ev


CASES = {
    # AC-1
    "happy": [prop(), rv(R1), rv(R2)],
    # AC-2
    "self_review": [prop(), rv({"kind": "human", "id": "Author-A"}),
                    rv({"kind": "human", "id": "rev-one"})],
    "dup_reviewer": [prop(), rv(R1), rv(R1)],
    "not_in_roster": [prop(), rv({"kind": "human", "id": "rev-three"}), rv({"kind": "human", "id": "nobody"}),
                      rv({"kind": "human", "id": "rev-five"})],
    "agent_reviewer": [prop(), rv({"kind": "agent", "id": "rev-one"}), rv({"kind": "script", "id": "rev-one"})],
    "no_attestation": [prop(), rv(R1, attest="none"), rv(R1, attest=[["syn-std-a", "2001"]]),
                       rv(R1, attest=[["syn-std-a", "2001"], ["syn-std-b", "2001"]])],
    # AC-3
    "agent_authored": [prop(author=AGENT), rv(R1), prop(), rv(R1), rv(R2)],
    # AC-4
    "reset": [prop(), rv(R1), rv(R2), rv(R1), prop("supported_v2"), rv(R1, reviewed="supported")],
    # AC-5
    "placeholder": [prop("placeholder"), rv(R1, attest="placeholder"), rv(R2, attest="cited")],
    "placeholder_on_supported": [prop(), rv(R1, attest="placeholder")],
    # AC-6 and REQ-005
    "idr": [prop(), rv(R1),
            rv(R2, decision="request_changes",
               idr={"question_code": "zone_shape", "positions": ["x" * 281]}),
            rv(R2, decision="request_changes",
               idr={"question_code": "zone_shape", "positions": ["y" * 280, "A second reading."]}),
            rv(R1), rv(R2)],
    # D-045: content digest covers synthetic; review state never changes it; stale review.
    "synthetic_flip": [prop(), rv(R1), rv(R2), prop("supported_synthetic_flip"),
                       rv(R1, reviewed="supported"), rv(R1)],
    "same_person_two_principals": [prop(author={"kind": "human", "id": "author-four"}),
                                   rv({"kind": "human", "id": "author-four-alt"}), rv(R1)],
    "key_change": [prop(), prop("other_key")],
    # Review findings on #320: a row file cannot carry its own sign-off; no backdated review.
    "forged_review": [prop("supported_review_only"), prop(), rv(R1), rv(R2)],
    "backdated": [prop(), rv(R1, at="2026-11-19"), rv(R1), rv(R2, at="2026-11-19"),
                  rv(R2, at="2026-11-21")],
}

DIGESTS = {name: content_digest(row) for name, row in sorted(ROWS.items())}


def files():
    out = {
        "state_rows.json": {"rows": ROWS, "content_digests": DIGESTS},
        "state_roster.json": ROSTER,
    }
    for name, events in CASES.items():
        out[name + ".json"] = {"case": name, "roster": "state_roster.json", "rows": "state_rows.json",
                               "events": events, "expected": run(events)}
    return {k: (json.dumps(v, indent=1, sort_keys=False) + "\n").encode("utf-8") for k, v in out.items()}


def sanity():
    # REQ-GDT.RES.STATUS-001 / AC-10 of GDT.RES.STATUS: notes and synthetic count, review does not.
    assert DIGESTS["supported"] != DIGESTS["supported_v2"]
    assert DIGESTS["supported"] == DIGESTS["supported_review_only"]
    assert DIGESTS["supported"] != DIGESTS["supported_synthetic_flip"]


def main():
    sanity()
    want = files()
    if "--check" in sys.argv[1:]:
        bad = [n for n, b in sorted(want.items()) if not (HERE / n).is_file() or (HERE / n).read_bytes() != b]
        if bad:
            sys.exit("stale fixtures: " + ", ".join(bad))
        print("ok", len(want), "files")
    else:
        for n, b in sorted(want.items()):
            (HERE / n).write_bytes(b)
        print("wrote", len(want), "files")


if __name__ == "__main__":
    main()
