#!/usr/bin/env python3
"""Independent reference for the interpretation status register (GDT.RES.STATUS).

Stdlib only. It writes the synthetic roster, conflict-of-interest inputs, journal extracts,
expected registers, `gdt-published/v1` lists and expected answers under this folder, labelling
every outcome from the leaf's rules (REQ-GDT.RES.STATUS-001 .. 010) and the review rules of
GDT.RES.ROWFLOW.STATE. The Rust crate is never run to produce an expected value.

    python make_cases.py           # rewrite the fixtures
    python make_cases.py --check   # verify the committed fixtures are current

Everything here is invented: keys, designators, editions, clause numbers, reviewer IDs,
principal identities, organisation IDs, profile IDs and digests. No standards text, no real
names. CC0.

Rules this reference applies:

- A review entry is applied only if STATE would accept it, checked in STATE's order: revision
  not signed_off; author human; entry principal human; reviewed digest = content digest;
  reviewer is not the author; the principal holds a roster role for the dialect on the day and
  that role's ID is the one the record names; the author does not hold that ID; no second
  live approval from the same reviewer; every cited (designator, edition) attested.
  request_changes clears the live approvals; two live approvals make STATE's signed_off,
  after which further reviews are refused.
- An approval counts (REQ-003) only if live, from a human entry, naming the digest, from a
  reviewer the reviewer register lists (when one is loaded), not recused from the rule, and
  with no other live approver of the same organisation ID.
- Status (REQ-004/005): earlier revisions provisional/superseded; latest agent-authored
  revision draft; latest with two counted approvals from distinct reviewers signed_off;
  otherwise provisional/awaiting_review. Profiles (working branch) provisional/no_signoff_path,
  revision 1 (REQ-006).
- Verification (REQ-007/010): roster digest listed, byte-identical rebuild, register digest
  listed with the same roster digest; for evaluate it must be the last entry.
"""

import copy
import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
DAY = "2026-11-20"
JOURNAL_SCHEMA = "gdt-rowflow-journal/v1"
PUBLISHED_SCHEMA = "gdt-published/v1"
REGISTER_SCHEMA = "gdt-rule-status/v1"
PROPOSE = "gdt.propose_dmt_row@1"
REVIEW = "gdt.review_dmt_row@1"
PUBLISH = "gdt.publish_defaults_profile@1"


def canon(v):
    s = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert s.isascii(), "reference only covers ASCII documents"
    return s


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def content_digest(row):
    return sha(canon({k: v for k, v in row.items() if k != "review"}).encode("utf-8"))


# ------------------------------------------------------------------ synthetic rows

STEPS = [
    {"kind": "partition", "alg": "syn.partition.whole_feature@0.1.0", "params": {}},
    {"kind": "extraction", "alg": "syn.extract.all_points@0.1.0", "params": {"min_points": "4"}},
    {"kind": "association", "alg": "syn.assoc.min_zone@0.1.0", "params": {"constrained": True}},
    {"kind": "evaluation", "alg": "syn.eval.max_deviation@0.1.0", "params": {}},
]

ROW_A = {
    "key": "dmt:asme/flatness/none@1",
    "dialect": "asme",
    "editions": ["syn-a-2001", "syn-b-2010"],
    "characteristic": "flatness",
    "modifiers": [],
    "clauses": [
        {"designator": "syn-std-a", "edition": "2001", "clause": "4.2"},
        {"designator": "syn-std-b", "edition": "2010", "clause": "7"},
    ],
    "variants": [{"variant": "surface", "steps": STEPS}],
    "default_variant": "surface",
    "actual_value": "av.synthetic.flatness@1",
    "decision_rule_hint": "dr.synthetic.default@1",
    "ors_cases": ["ors:synthetic/flatness.001"],
    "status": "supported",
    "limitations": [],
    "review": {"state": "draft"},
    "notes": "Synthetic row for status register tests; not an interpretation of any standard.",
    "synthetic": True,
}


def variant(**changes):
    row = copy.deepcopy(ROW_A)
    row.update(changes)
    return row


ROWS = {
    "a": ROW_A,
    "a_notes": variant(notes="Synthetic row, second wording of the note."),
    "a_review": variant(review={"state": "signed_off", "reviewers": ["r1", "r2"]}),
    "a_flip": variant(synthetic=False),
}

# ------------------------------------------------------------------ roster and conflict inputs


def roster_entry(rid, principal, name):
    return {"id": rid, "display_name": name, "principals": ["human:" + principal], "dialects": ["asme"],
            "basis": "community", "valid_from": "2026-01-01"}


ROSTER = {
    "version": "dmt-reviewers/v1",
    "entries": [
        roster_entry("r1", "rev-one", "Fictional reviewer one"),
        roster_entry("r2", "rev-two", "Fictional reviewer two"),
        roster_entry("r3", "rev-three", "Fictional reviewer three"),
        roster_entry("r4", "rev-four", "Fictional reviewer four"),
        roster_entry("r5", "rev-five", "Fictional reviewer five"),
    ],
}

ROSTER_FOREIGN = copy.deepcopy(ROSTER)
ROSTER_FOREIGN["entries"].append(roster_entry("r9", "rev-nine", "Fictional reviewer nine"))


def roster_digest(doc):
    entries = {}
    for e in doc["entries"]:
        o = {k: e[k] for k in ("basis", "display_name", "valid_from", "valid_until") if k in e}
        o["principals"] = sorted(e["principals"])
        o["dialects"] = sorted(e["dialects"])
        entries[e["id"]] = o
    return sha(canon({"entries": entries, "version": doc["version"]}).encode("utf-8"))


ROSTER_DIGEST = roster_digest(ROSTER)
FOREIGN_DIGEST = roster_digest(ROSTER_FOREIGN)

# Reviewer register (gdt-reviewer-register/v1): organisation IDs and recusals only; no names.
# r5 is deliberately missing: an approver the register does not list never counts.
REVIEWERS = [
    {"id": "r1", "org_id": "org-1", "recusals": []},
    {"id": "r2", "org_id": "org-2", "recusals": []},
    {"id": "r3", "org_id": "org-1", "recusals": []},
    {"id": "r4", "org_id": "org-3", "recusals": ["dmt:asme/flatness/none@1"]},
]

PROFILE_ID = "dp:asme-syn-2001@1.0.0"
PROFILE_DIGEST = sha(b"synthetic profile content one")
PROFILE_REF = PROFILE_ID + "#" + PROFILE_DIGEST
AGENT_PROFILE_REF = "dp:asme-syn-2001@1.1.0#" + sha(b"synthetic profile content two")
PROFILES = [{"id": PROFILE_REF, "sponsor_org": "org-9"}]


def toml_reviewers():
    out = ['schema = "gdt-reviewer-register/v1"', ""]
    for r in REVIEWERS:
        recusals = ", ".join('"%s"' % x for x in r["recusals"])
        out += ["[[reviewer]]", 'id = "%s"' % r["id"], 'org_id = "%s"' % r["org_id"],
                "recusals = [%s]" % recusals, "credit_name = false", "credit_org = false", ""]
    return "\n".join(out)


def toml_profiles():
    out = ['schema = "gdt-profiles/v1"', ""]
    for p in PROFILES:
        out += ["[[profile]]", 'id = "%s"' % p["id"], 'sponsor_org = "%s"' % p["sponsor_org"],
                'state = "active"', "default = true", ""]
    return "\n".join(out)


def coi_digest(loaded):
    profiles = {p["id"].split("#")[0]: {"sponsor_org": p["sponsor_org"]} for p in PROFILES} if loaded else {}
    reviewers = None
    if loaded:
        reviewers = {r["id"].lower(): {"org_id": r["org_id"], "recusals": sorted(set(r["recusals"]))}
                     for r in REVIEWERS}
    return sha(canon({"profiles": profiles, "reviewers": reviewers}).encode("utf-8"))


def coi_of(loaded):
    if not loaded:
        return None
    return {r["id"]: r for r in REVIEWERS}


# ------------------------------------------------------------------ the journal and STATE's rules

def has_role(principal_id):
    ident = "human:" + principal_id.lower()
    for e in ROSTER["entries"]:
        if ident in e["principals"] and "asme" in e["dialects"] and e["valid_from"] <= DAY:
            return e["id"]
    return None


def cited(row):
    seen = []
    for c in row["clauses"]:
        pair = (c["designator"], c["edition"])
        if pair not in seen:
            seen.append(pair)
    return sorted(seen)


class Journal:
    """Builds a journal extract and, alongside, the reference's own model of each revision."""

    def __init__(self, case):
        self.case = case
        self.entries = []
        self.keys = {}  # key -> [revision model]
        self.profiles = {}  # profile id -> (ref, entry)

    def _entry(self, command, kind, pid, record, row=None):
        i = len(self.entries)
        e = {"hash": sha(("%s/%d" % (self.case, i)).encode("ascii")),
             "utc": "%sT09:%02d:00Z" % (DAY, i), "command": command,
             "principal": {"kind": kind, "id": pid}, "record": record}
        if row is not None:
            e["row"] = row
        self.entries.append(e)
        return e

    def propose(self, row_name, kind="human", pid="author-one"):
        row = ROWS[row_name]
        revs = self.keys.setdefault(row["key"], [])
        prev = revs[-1] if revs else None
        n = 1 if prev is None else prev["revision"] + 1
        cd = content_digest(row)
        derived = None if prev is None else {"content_digest": prev["cd"], "revision": str(prev["revision"])}
        record = {"at": DAY, "author_kind": kind, "content_digest": cd, "derived_from": derived,
                  "event": "propose", "from": None, "key": row["key"],
                  "provenance": "human" if kind == "human" else "agent", "revision": str(n), "to": "draft"}
        e = self._entry(PROPOSE, kind, pid, record, row)
        revs.append({"revision": n, "cd": cd, "row": row, "author_kind": kind, "author_id": pid,
                     "state": "draft", "applied": [], "propose": e})

    def review(self, rid, pid, decision="approve", kind="human", named=None):
        key = ROW_A["key"]
        rev = self.keys[key][-1]
        reviewed = rev["cd"] if named is None else content_digest(ROWS[named])
        live = live_of(rev)
        if decision == "approve":
            after = "signed_off" if len(set(live + [rid])) >= 2 else "under_review"
        else:
            after = "draft"
        record = {"at": DAY,
                  "attestations": [{"designator": d, "edition": ed, "kind": "edition_held"} for d, ed in cited(rev["row"])],
                  "content_digest": rev["cd"], "event": decision, "from": rev["state"], "idr": None,
                  "key": key, "reviewed_digest": reviewed, "reviewer": rid, "revision": str(rev["revision"]),
                  "roster_digest": ROSTER_DIGEST, "to": after}
        e = self._entry(REVIEW, kind, pid, record)
        if accepts(rev, e, rid, decision, kind, pid, reviewed):
            rev["applied"].append((rid, decision, e))
            rev["state"] = after

    def publish(self, ref, kind="human", pid="author-one"):
        branch = "working" if kind == "human" else "agent"
        record = {"branch": branch, "command": PUBLISH, "file": "profile-syn.json", "principal": kind, "ref": ref}
        e = self._entry(PUBLISH, kind, pid, record)
        pid_ = ref.split("#")[0]
        if branch == "working" and pid_ not in self.profiles:
            self.profiles[pid_] = (ref, e)

    def extract(self):
        return {"schema": JOURNAL_SCHEMA, "entries": self.entries}


def live_of(rev):
    live = []
    for rid, decision, _ in rev["applied"]:
        if decision == "request_changes":
            live = []
        else:
            live.append(rid)
    return live


def accepts(rev, e, rid, decision, kind, pid, reviewed):
    """STATE's checks, in its order; True when the review would have been journaled."""
    if rev["state"] == "signed_off":
        return False
    if rev["author_kind"] != "human":
        return False
    if kind != "human":
        return False
    if reviewed != rev["cd"]:
        return False
    if pid.lower() == rev["author_id"].lower():
        return False
    role = has_role(pid)
    if role is None or role != rid:
        return False
    if has_role(rev["author_id"]) == rid:
        return False
    if decision == "approve" and rid in live_of(rev):
        return False
    return True


# ------------------------------------------------------------------ the register

def counted(rule, rev, coi):
    live = []
    for rid, decision, e in rev["applied"]:
        if decision == "request_changes":
            live = []
        elif e["principal"]["kind"] == "human" and e["record"]["reviewed_digest"] == rev["cd"]:
            live.append((rid, e))
    out = []
    for rid, e in live:
        if coi is not None:
            mine = coi.get(rid)
            if mine is None or rule in mine["recusals"]:
                continue
            if any(o != rid and coi.get(o) is not None and coi[o]["org_id"] == mine["org_id"] for o, _ in live):
                continue
        out.append({"at": e["utc"], "journal_entry": e["hash"], "reviewer": rid})
    return sorted(out, key=lambda a: a["reviewer"])


def register(journal, current_digest, loaded):
    coi = coi_of(loaded)
    entries = []
    for key, revs in journal.keys.items():
        for i, rev in enumerate(revs):
            approvals = counted(key, rev, coi)
            if i + 1 < len(revs):
                status, reason = "provisional", "superseded"
            elif rev["author_kind"] != "human":
                status, reason = "draft", None
            elif len({a["reviewer"] for a in approvals}) >= 2:
                status, reason = "signed_off", None
            else:
                status, reason = "provisional", "awaiting_review"
            entries.append({"approvals": approvals, "author_kind": rev["author_kind"], "content_digest": rev["cd"],
                            "kind": "dmt_row", "reason": reason, "revised_at": rev["propose"]["utc"],
                            "revision": rev["revision"], "rule": key, "status": status})
    for pid, (ref, e) in journal.profiles.items():
        entries.append({"approvals": [], "author_kind": e["principal"]["kind"], "content_digest": ref.split("#")[1],
                        "kind": "profile", "reason": "no_signoff_path", "revised_at": e["utc"], "revision": 1,
                        "rule": pid, "status": "provisional"})
    entries.sort(key=lambda x: (x["rule"], x["revision"]))
    built = {"coi_digest": coi_digest(loaded), "journal_head": journal.entries[-1]["hash"],
             "roster_digest": current_digest}
    return {"built_from": built, "entries": entries, "schema": REGISTER_SCHEMA}


def answers(reg, verified):
    """status_of for every entry's (rule, digest) plus two probes that are not listed."""
    probes = [(e["rule"], e["content_digest"]) for e in reg["entries"]]
    probes.append(("dmt:asme/unknown_rule/none@1", content_digest(ROW_A)))
    probes.append((ROW_A["key"], sha(b"a digest no revision has")))
    out = []
    for rule, digest in probes:
        if not verified:
            status, reason = "provisional", "unverified_register"
        else:
            hits = [e for e in reg["entries"] if e["rule"] == rule and e["content_digest"] == digest]
            if hits:
                status, reason = hits[-1]["status"], hits[-1]["reason"]
            else:
                status, reason = "provisional", "not_in_register"
        out.append({"rule": rule, "digest": digest, "status": status, "reason": reason})
    return out


def published(regs):
    return {"schema": PUBLISHED_SCHEMA,
            "entries": [{"register_digest": sha(b), "roster_digest": r} for b, r in regs]}


def verify(file_bytes, rebuilt_bytes, current_digest, pub, purpose):
    listed_rosters = {e["roster_digest"] for e in pub["entries"]}
    if current_digest not in listed_rosters:
        return "foreign_roster"
    if file_bytes != rebuilt_bytes:
        return "register_mismatch"
    digest = sha(file_bytes)
    idx = [i for i, e in enumerate(pub["entries"]) if e["register_digest"] == digest]
    if not idx:
        return "register_not_published"
    if pub["entries"][idx[0]]["roster_digest"] != current_digest:
        return "foreign_roster"
    if purpose == "evaluate" and idx[0] + 1 != len(pub["entries"]):
        return "stale_register"
    return "verified"


# ------------------------------------------------------------------ cases

def happy_actions(j):
    j.propose("a")
    j.review("r1", "rev-one")
    j.review("r2", "rev-two")
    j.publish(PROFILE_REF)
    j.publish(AGENT_PROFILE_REF, kind="agent", pid="agent-one")


CASES = {
    # AC-1, AC-6 (profile without sign-off key), AC-11.
    "happy": happy_actions,
    # AC-2.
    "one_approval": lambda j: (j.propose("a"), j.review("r1", "rev-one")),
    # AC-3: agent-authored latest revision; the review attempt is refused.
    "agent_draft": lambda j: (j.propose("a", kind="agent", pid="agent-one"), j.review("r1", "rev-one")),
    # AC-3: a human re-proposal (derived_from the agent revision).
    "agent_rewritten": lambda j: (j.propose("a", kind="agent", pid="agent-one"), j.propose("a")),
    # AC-4.
    "changed": lambda j: (j.propose("a"), j.review("r1", "rev-one"), j.review("r2", "rev-two"), j.propose("a_notes")),
    # AC-5: approvals naming another digest.
    "wrong_digest": lambda j: (j.propose("a"), j.review("r1", "rev-one", named="a_notes"),
                               j.review("r2", "rev-two", named="a_notes")),
    # AC-5: an approval carried by an agent principal.
    "agent_approval": lambda j: (j.propose("a"), j.review("r1", "rev-one"), j.review("r2", "rev-two", kind="agent")),
    # AC-5: both approvers with one organisation ID (STATE alone would call this signed_off).
    "same_org": lambda j: (j.propose("a"), j.review("r1", "rev-one"), j.review("r3", "rev-three")),
    # AC-5: an approver recused from the rule.
    "recused": lambda j: (j.propose("a"), j.review("r2", "rev-two"), j.review("r4", "rev-four")),
    # REQ-003, failing closed: an approver the reviewer register does not list.
    "unregistered": lambda j: (j.propose("a"), j.review("r1", "rev-one"), j.review("r5", "rev-five")),
    # A request_changes after one approval clears it; a later approval alone is not enough.
    "request_changes": lambda j: (j.propose("a"), j.review("r1", "rev-one"),
                                  j.review("r2", "rev-two", decision="request_changes"), j.review("r1", "rev-one")),
    # AC-10: the signed-off row re-proposed with only `synthetic` flipped.
    "synthetic_flip": lambda j: (happy_actions(j), j.propose("a_flip")),
}


def case_files(name):
    j = Journal(name)
    CASES[name](j)
    reg = register(j, ROSTER_DIGEST, True)
    b = canon(reg).encode("utf-8")
    pub = published([(b, ROSTER_DIGEST)])
    item = {"file": "register.json", "current": "roster.json", "head": reg["built_from"]["journal_head"],
            "genuine": True, "digest": sha(b),
            "verify": {p: verify(b, b, ROSTER_DIGEST, pub, p) for p in ("evaluate", "replay")}}
    item["answers"] = answers(reg, item["verify"]["evaluate"] == "verified")
    expected = {"coi": "loaded", "rosters": ["roster.json"], "registers": [item]}
    return {"journal.json": pretty(j.extract()), "register.json": b, "published.json": pretty(pub),
            "expected.json": pretty(expected)}


def same_org_no_coi():
    # The spec's interim rule: with no conflict inputs every approval that passes the other
    # checks counts, so STATE's two approvals count. The coi_digest records the empty inputs.
    j = Journal("same_org_no_coi")
    CASES["same_org"](j)
    reg = register(j, ROSTER_DIGEST, False)
    b = canon(reg).encode("utf-8")
    pub = published([(b, ROSTER_DIGEST)])
    v = {p: verify(b, b, ROSTER_DIGEST, pub, p) for p in ("evaluate", "replay")}
    item = {"file": "register.json", "current": "roster.json", "head": reg["built_from"]["journal_head"],
            "genuine": True, "digest": sha(b), "verify": v, "answers": answers(reg, v["evaluate"] == "verified")}
    return {"journal.json": pretty(j.extract()), "register.json": b, "published.json": pretty(pub),
            "expected.json": pretty({"coi": "none", "rosters": ["roster.json"], "registers": [item]})}


def forged():
    # AC-7: the happy register with its provisional profile entry edited to signed_off.
    j = Journal("happy")
    happy_actions(j)
    genuine = register(j, ROSTER_DIGEST, True)
    gb = canon(genuine).encode("utf-8")
    bad = copy.deepcopy(genuine)
    edited = [e for e in bad["entries"] if e["status"] == "provisional"]
    assert edited, "the happy register needs a provisional entry to forge"
    edited[0]["status"], edited[0]["reason"] = "signed_off", None
    b = canon(bad).encode("utf-8")
    pub = published([(gb, ROSTER_DIGEST)])
    v = {p: verify(b, gb, ROSTER_DIGEST, pub, p) for p in ("evaluate", "replay")}
    item = {"file": "register.json", "current": "roster.json", "head": bad["built_from"]["journal_head"],
            "genuine": False, "digest": sha(b), "verify": v, "answers": answers(bad, False)}
    return {"journal.json": pretty(j.extract()), "register.json": b, "published.json": pretty(pub),
            "expected.json": pretty({"coi": "loaded", "rosters": ["roster.json"], "registers": [item]})}


def foreign_roster():
    # AC-8: rebuilt correctly from the journal, but recording a roster that is not published.
    j = Journal("happy")
    happy_actions(j)
    genuine = canon(register(j, ROSTER_DIGEST, True)).encode("utf-8")
    reg = register(j, FOREIGN_DIGEST, True)
    b = canon(reg).encode("utf-8")
    pub = published([(genuine, ROSTER_DIGEST)])
    v = {p: verify(b, b, FOREIGN_DIGEST, pub, p) for p in ("evaluate", "replay")}
    item = {"file": "register.json", "current": "roster_foreign.json", "head": reg["built_from"]["journal_head"],
            "genuine": True, "digest": sha(b), "verify": v, "answers": answers(reg, False)}
    return {"journal.json": pretty(j.extract()), "register.json": b, "published.json": pretty(pub),
            "expected.json": pretty({"coi": "loaded", "rosters": ["roster.json", "roster_foreign.json"],
                                     "registers": [item]})}


def currency():
    # AC-12: R1 built after the first approval, R2 at the head; published lists R1 then R2.
    j1 = Journal("happy")
    j1.propose("a")
    j1.review("r1", "rev-one")
    r1 = register(j1, ROSTER_DIGEST, True)
    j2 = Journal("happy")
    happy_actions(j2)
    r2 = register(j2, ROSTER_DIGEST, True)
    b1, b2 = canon(r1).encode("utf-8"), canon(r2).encode("utf-8")
    pub = published([(b1, ROSTER_DIGEST), (b2, ROSTER_DIGEST)])
    items = []
    for fname, reg, b in (("register_r1.json", r1, b1), ("register_r2.json", r2, b2)):
        v = {p: verify(b, b, ROSTER_DIGEST, pub, p) for p in ("evaluate", "replay")}
        items.append({"file": fname, "current": "roster.json", "head": reg["built_from"]["journal_head"],
                      "genuine": True, "digest": sha(b), "verify": v,
                      "answers": answers(reg, v["evaluate"] == "verified")})
    return {"journal.json": pretty(j2.extract()), "register_r1.json": b1, "register_r2.json": b2,
            "published.json": pretty(pub),
            "expected.json": pretty({"coi": "loaded", "rosters": ["roster.json"], "registers": items})}


def pretty(v):
    return (json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def files():
    out = {
        "roster.json": pretty(ROSTER),
        "roster_foreign.json": pretty(ROSTER_FOREIGN),
        "reviewers.toml": toml_reviewers().encode("utf-8"),
        "profiles.toml": toml_profiles().encode("utf-8"),
        "rows.json": pretty({"rows": ROWS, "content_digests": {k: content_digest(v) for k, v in ROWS.items()},
                             "roster_digest": ROSTER_DIGEST, "foreign_roster_digest": FOREIGN_DIGEST,
                             "coi_digest": coi_digest(True), "coi_digest_none": coi_digest(False)}),
    }
    per_case = {name: case_files(name) for name in CASES}
    per_case["same_org_no_coi"] = same_org_no_coi()
    per_case["forged"] = forged()
    per_case["foreign_roster"] = foreign_roster()
    per_case["currency"] = currency()
    for case, fs in per_case.items():
        for fname, data in fs.items():
            out[case + "/" + fname] = data
    return out


def sanity(out):
    # Hand-labelled expectations the generated data must meet (the ACs, restated).
    def reg(case, f="register.json"):
        return json.loads(out[case + "/" + f])

    def row_status(case, revision=None):
        es = [e for e in reg(case)["entries"] if e["kind"] == "dmt_row"]
        e = es[-1] if revision is None else [x for x in es if x["revision"] == revision][0]
        return e["status"], e["reason"], [a["reviewer"] for a in e["approvals"]]

    assert row_status("happy") == ("signed_off", None, ["r1", "r2"])
    assert row_status("one_approval") == ("provisional", "awaiting_review", ["r1"])
    assert row_status("agent_draft") == ("draft", None, [])
    assert row_status("agent_rewritten") == ("provisional", "awaiting_review", [])
    assert row_status("agent_rewritten", 1)[:2] == ("provisional", "superseded")
    assert row_status("changed") == ("provisional", "awaiting_review", [])
    assert row_status("changed", 1) == ("provisional", "superseded", ["r1", "r2"])
    for case in ("wrong_digest", "agent_approval", "same_org", "recused", "unregistered", "request_changes"):
        assert row_status(case)[0] == "provisional", case
    assert row_status("same_org_no_coi") == ("signed_off", None, ["r1", "r3"])
    assert row_status("synthetic_flip") == ("provisional", "awaiting_review", [])
    assert row_status("synthetic_flip", 1) == ("provisional", "superseded", ["r1", "r2"])
    profiles = [e for e in reg("happy")["entries"] if e["kind"] == "profile"]
    assert [(e["rule"], e["status"], e["reason"]) for e in profiles] == [(PROFILE_ID, "provisional", "no_signoff_path")]
    exp = lambda case: json.loads(out[case + "/expected.json"])["registers"]
    assert [r["verify"] for r in exp("forged")] == [{"evaluate": "register_mismatch", "replay": "register_mismatch"}]
    assert [r["verify"] for r in exp("foreign_roster")] == [{"evaluate": "foreign_roster", "replay": "foreign_roster"}]
    assert [r["verify"] for r in exp("currency")] == [{"evaluate": "stale_register", "replay": "verified"},
                                                      {"evaluate": "verified", "replay": "verified"}]
    assert all(a["status"] != "signed_off" for r in exp("forged") for a in r["answers"])
    assert content_digest(ROWS["a"]) != content_digest(ROWS["a_notes"])
    assert content_digest(ROWS["a"]) == content_digest(ROWS["a_review"])
    assert content_digest(ROWS["a"]) != content_digest(ROWS["a_flip"])
    # No public fixture carries a display name, organisation name or contact detail in the
    # register: reviewers appear by roster ID only.
    for k, v in out.items():
        if k.rsplit("/", 1)[-1].startswith("register"):
            text = v.decode("utf-8")
            assert "rev-" not in text and "Fictional" not in text and "@" not in text.replace("@1", ""), k


def main():
    out = files()
    sanity(out)
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(out.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        if stale:
            sys.exit("stale fixtures: " + ", ".join(stale))
        print("ok", len(out), "files")
    else:
        print("wrote", len(out), "files")


if __name__ == "__main__":
    main()
