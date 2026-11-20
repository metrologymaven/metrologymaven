#!/usr/bin/env python3
"""Independent reference for the interpretation block of a ResultRecord (GDT.CHAR.INTERP).

Stdlib only. It writes synthetic DMT rows, `GDT.RES.ROWFLOW` journal extracts, status
registers (`gdt-rule-status/v1`), `gdt-published/v1` lists and `cases.json` (the inputs of
each case and the interpretation block, error or replay outcome expected), labelling every
outcome from the leaf's rules:

- REQ-001/002: one rule use per distinct (kind, rule ID, content digest) for the DMT rows the
  evaluation used plus the defaults profile, sorted by (kind, rule ID, digest); a row's digest
  is SHA-256 of the canonical JSON of the row without `review`, recomputed from the row
  itself (whatever review metadata it carries); the profile's digest is SHA-256 of the
  profile document's canonical JSON; the profile's rule ID is `dp:<dialect>-<edition>@<ver>`.
- REQ-003: the published list must extend the pinned prior (else `published_unreadable`);
  the register must verify (rules of GDT.RES.STATUS REQ-007/010, reused from the status
  reference) for `evaluate` (the latest entry) on a new evaluation, `replay` on replay.
  Unverified: every rule provisional/unverified_register. No register: provisional/
  no_register. Verified: the register entry for (rule, digest), latest revision first;
  absent: provisional/not_in_register.
- Revision: the register entry's; 1 for a profile without one (profiles are immutable,
  revision always 1); null for a row without one.
- REQ-004: signed_off exactly when every rule is. REQ-005: a draft rule refuses the record
  with rule_not_admitted naming it. REQ-006: no rows, the primary row absent or no profile
  is incomplete_result{interpretation_rules}.
- `interpretation_register`: the digest of the register file consulted, or `none`.

The register fold, verification and roster/conflict inputs come from the status reference
`../status/make_cases.py` (also stdlib Python, also independent of the Rust code); the
Rust crate is never run to produce an expected value.

    python make_cases.py           # rewrite the fixtures
    python make_cases.py --check   # verify the committed fixtures are current

Everything here is invented: keys, designators, editions, clause numbers, reviewer IDs,
principal identities and digests. No standards text. CC0.
"""

import copy
import importlib.util
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("status_reference", HERE.parent / "status" / "make_cases.py")
status = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(status)

canon, sha, content_digest = status.canon, status.sha, status.content_digest
ROSTER_DIGEST, FOREIGN_DIGEST = status.ROSTER_DIGEST, status.FOREIGN_DIGEST

# ------------------------------------------------------------------ profile

PROFILE_FILE = HERE.parent / "dprof" / "asme_placeholder_1.0.0.json"
PROFILE_DOC = json.loads(PROFILE_FILE.read_text(encoding="utf-8"))
PROFILE_DIGEST = sha(canon(PROFILE_DOC).encode("utf-8"))
PROFILE_ID = "dp:%s-%s@%s" % (PROFILE_DOC["dialect"], PROFILE_DOC["edition"], PROFILE_DOC["version"])
PROFILE_REF = PROFILE_ID + "#" + PROFILE_DIGEST

# ------------------------------------------------------------------ rows


def row(key, characteristic, modifiers, note):
    r = copy.deepcopy(status.ROW_A)
    r.update(key=key, characteristic=characteristic, modifiers=modifiers, notes=note)
    return r


FLAT = status.ROW_A
POS = row("dmt:asme/position/mmc@1", "position", ["mmc"], "Synthetic position row with bonus; not an interpretation.")
DRF = row("dmt:asme/datum_frame/rmb@1", "datum_frame", ["rmb"], "Synthetic planar datum frame row; not an interpretation.")
SIZE = row("dmt:asme/size/none@1", "size", [], "Synthetic size row; not an interpretation.")
TAMPERED = copy.deepcopy(FLAT)
TAMPERED["variants"][0]["steps"][1]["params"]["min_points"] = "5"
# The edited row still claims the signed-off revision in its review metadata.
TAMPERED["review"] = {"state": "signed_off", "reviewers": ["r1", "r2"]}

ROWS = {"flat": FLAT, "pos": POS, "drf": DRF, "size": SIZE, "flat_tampered": TAMPERED}
status.ROWS.update(ROWS)

# ------------------------------------------------------------------ journals


class Journal(status.Journal):
    """The status reference's journal, with reviews of any row key."""

    def review_key(self, key, rid, pid):
        rev = self.keys[key][-1]
        live = status.live_of(rev)
        after = "signed_off" if len(set(live + [rid])) >= 2 else "under_review"
        record = {"at": status.DAY,
                  "attestations": [{"designator": d, "edition": ed, "kind": "edition_held"}
                                   for d, ed in status.cited(rev["row"])],
                  "content_digest": rev["cd"], "event": "approve", "from": rev["state"], "idr": None,
                  "key": key, "reviewed_digest": rev["cd"], "reviewer": rid, "revision": str(rev["revision"]),
                  "roster_digest": ROSTER_DIGEST, "to": after}
        e = self._entry(status.REVIEW, "human", pid, record)
        if status.accepts(rev, e, rid, "approve", "human", pid, rev["cd"]):
            rev["applied"].append((rid, "approve", e))
            rev["state"] = after

    def signed(self, name):
        self.propose(name)
        self.review_key(ROWS[name]["key"], "r1", "rev-one")
        self.review_key(ROWS[name]["key"], "r2", "rev-two")


def register_bytes(j, current=ROSTER_DIGEST):
    reg = status.register(j, current, True)
    return reg, canon(reg).encode("utf-8")


def published(*regs):
    return status.published([(b, ROSTER_DIGEST) for b in regs])


# ------------------------------------------------------------------ the oracle


def verification(file_bytes, genuine_bytes, current, pub, prior, purpose):
    if pub["entries"][: len(prior["entries"])] != prior["entries"]:
        return "published_unreadable"
    return status.verify(file_bytes, genuine_bytes, current, pub, purpose)


def interp(rows, profile, reg, code, file_bytes, primary):
    """The expected block, or the expected error."""
    if not rows or primary not in [r["key"] for r in rows] or not profile:
        return {"error": "incomplete_result{interpretation_rules}"}
    uses = sorted({("dmt_row", r["key"], content_digest(r)) for r in rows} | {("profile", PROFILE_ID, PROFILE_DIGEST)})
    rules = []
    for kind, rule, digest in uses:
        fallback = 1 if kind == "profile" else None
        if code == "none":
            st, reason, revision = "provisional", "no_register", fallback
        elif code != "verified":
            st, reason, revision = "provisional", "unverified_register", fallback
        else:
            hits = [e for e in reg["entries"] if e["rule"] == rule and e["content_digest"] == digest]
            if hits:
                st, reason, revision = hits[-1]["status"], hits[-1]["reason"], hits[-1]["revision"]
            else:
                st, reason, revision = "provisional", "not_in_register", fallback
        if st == "draft":
            return {"error": "rule_not_admitted{%s}" % rule}
        rules.append({"content_digest": digest, "kind": kind, "reason": reason, "revision": revision,
                      "rule": rule, "status": st})
    label = "signed_off" if all(r["status"] == "signed_off" for r in rules) else "provisional"
    return {"interpretation": label, "interpretation_register": sha(file_bytes) if file_bytes is not None else "none",
            "interpretation_rules": rules, "verification": code}


def pretty(v):
    return (json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def files():
    out = {"rows.json": pretty({"rows": ROWS, "content_digests": {k: content_digest(v) for k, v in ROWS.items()}}),
           "profile.json": pretty({"file": "../dprof/asme_placeholder_1.0.0.json", "id": PROFILE_ID,
                                   "digest": PROFILE_DIGEST, "ref": PROFILE_REF})}
    cases = {}

    def add(name, *, journal, register, genuine, published_file, prior_file, pub, prior, rows, primary,
            profile=True, current=ROSTER_DIGEST, rosters=("roster.json",), reg=None):
        code = "none" if register is None else verification(out[register], genuine, current, pub, prior, "evaluate")
        file_bytes = None if register is None else out[register]
        cases[name] = {"journal": journal, "register": register, "published": published_file, "prior": prior_file,
                       "rosters": list(rosters), "rows": rows, "primary": primary, "profile": profile,
                       "expect": interp([ROWS[r] for r in rows], profile, reg, code, file_bytes, primary)}

    # AC-1: one flatness row signed off, the profile published (always provisional:
    # GDT.RES.STATUS REQ-006 gives profiles no sign-off path yet).
    j = Journal("flatness_signed")
    j.signed("flat")
    j.publish(PROFILE_REF)
    reg_f, b_f = register_bytes(j)
    pub_f = published(b_f)
    out["flatness_signed/journal.json"] = pretty(j.extract())
    out["flatness_signed/register.json"] = b_f
    out["flatness_signed/published.json"] = pretty(pub_f)
    flat_inputs = dict(journal="flatness_signed/journal.json", genuine=b_f,
                       published_file="flatness_signed/published.json", prior_file="flatness_signed/published.json",
                       pub=pub_f, prior=pub_f)
    add("flatness_signed", register="flatness_signed/register.json", rows=["flat"], primary=FLAT["key"], reg=reg_f,
        **flat_inputs)

    # AC-2: position (MMC with bonus), datum frame (RMB) and size rows signed off.
    j = Journal("position_mmc_mixed")
    for n in ("pos", "drf", "size"):
        j.signed(n)
    j.publish(PROFILE_REF)
    reg_p, b_p = register_bytes(j)
    pub_p = published(b_p)
    out["position_mmc_mixed/journal.json"] = pretty(j.extract())
    out["position_mmc_mixed/register.json"] = b_p
    out["position_mmc_mixed/published.json"] = pretty(pub_p)
    add("position_mmc_mixed", journal="position_mmc_mixed/journal.json", register="position_mmc_mixed/register.json",
        genuine=b_p, published_file="position_mmc_mixed/published.json",
        prior_file="position_mmc_mixed/published.json", pub=pub_p, prior=pub_p, rows=["pos", "drf", "size"],
        primary=POS["key"], reg=reg_p)

    # AC-3: the flatness row with an edited chain and signed-off review metadata.
    add("tampered_row", register="flatness_signed/register.json", rows=["flat_tampered"], primary=FLAT["key"],
        reg=reg_f, **flat_inputs)

    # AC-4: the register edited to mark the profile signed_off; and one recording a roster
    # the published list does not name.
    forged = copy.deepcopy(reg_f)
    edited = [e for e in forged["entries"] if e["kind"] == "profile"]
    edited[0]["status"], edited[0]["reason"] = "signed_off", None
    out["forged_register/register.json"] = canon(forged).encode("utf-8")
    add("forged_register", register="forged_register/register.json", rows=["flat"], primary=FLAT["key"],
        reg=forged, **flat_inputs)
    _, b_foreign = register_bytes(j_foreign := _flat_journal(), FOREIGN_DIGEST)
    out["foreign_roster/register.json"] = b_foreign
    add("foreign_roster", register="foreign_roster/register.json", rows=["flat"], primary=FLAT["key"],
        current=FOREIGN_DIGEST, rosters=("roster.json", "roster_foreign.json"), reg=None,
        **dict(flat_inputs, genuine=b_foreign))
    assert j_foreign.extract() == json.loads(out["flatness_signed/journal.json"])

    # AC-5: the flatness row signed off, the position row agent-authored (draft).
    j = Journal("draft_row")
    j.signed("flat")
    j.propose("pos", kind="agent", pid="agent-one")
    j.publish(PROFILE_REF)
    reg_d, b_d = register_bytes(j)
    pub_d = published(b_d)
    out["draft_row/journal.json"] = pretty(j.extract())
    out["draft_row/register.json"] = b_d
    out["draft_row/published.json"] = pretty(pub_d)
    add("draft_row", journal="draft_row/journal.json", register="draft_row/register.json", genuine=b_d,
        published_file="draft_row/published.json", prior_file="draft_row/published.json", pub=pub_d, prior=pub_d,
        rows=["flat", "pos"], primary=FLAT["key"], reg=reg_d)

    # AC-6: no rows; the primary row absent; no profile.
    add("no_rule_list", register="flatness_signed/register.json", rows=[], primary=FLAT["key"], reg=reg_f,
        **flat_inputs)
    add("primary_absent", register="flatness_signed/register.json", rows=["pos"], primary=FLAT["key"], reg=reg_f,
        **flat_inputs)
    add("missing_profile", register="flatness_signed/register.json", rows=["flat"], primary=FLAT["key"],
        profile=False, reg=reg_f, **flat_inputs)

    # AC-7: no register loaded.
    add("no_register", register=None, rows=["flat"], primary=FLAT["key"], reg=None, **flat_inputs)

    # AC-13: R1 (one approval) published, then R2 (both approvals and the profile).
    j1 = Journal("currency")
    j1.propose("flat")
    j1.review_key(FLAT["key"], "r1", "rev-one")
    reg_1, b_1 = register_bytes(j1)
    j2 = Journal("currency")
    j2.signed("flat")
    j2.publish(PROFILE_REF)
    reg_2, b_2 = register_bytes(j2)
    v1, v2, rewritten = published(b_1), published(b_1, b_2), published(b_2)
    out["currency/journal.json"] = pretty(j2.extract())
    out["currency/register_r1.json"] = b_1
    out["currency/register_r2.json"] = b_2
    out["currency/published_v1.json"] = pretty(v1)
    out["currency/published_v2.json"] = pretty(v2)
    out["currency/published_rewritten.json"] = pretty(rewritten)
    cur = dict(journal="currency/journal.json", rows=["flat"], primary=FLAT["key"], prior_file="currency/published_v1.json",
               prior=v1)
    add("currency_r1_when_latest", register="currency/register_r1.json", genuine=b_1,
        published_file="currency/published_v1.json", pub=v1, reg=reg_1, **cur)
    add("currency_r1_stale", register="currency/register_r1.json", genuine=b_1,
        published_file="currency/published_v2.json", pub=v2, reg=reg_1, **cur)
    add("currency_r2_latest", register="currency/register_r2.json", genuine=b_2,
        published_file="currency/published_v2.json", pub=v2, reg=reg_2, **cur)
    add("currency_rewritten_list", register="currency/register_r2.json", genuine=b_2,
        published_file="currency/published_rewritten.json", pub=rewritten, reg=reg_2, **cur)
    # Replay of the record that pinned R1 while it was the latest, against the later list.
    replay_code = verification(b_1, b_1, ROSTER_DIGEST, v2, v1, "replay")
    assert replay_code == "verified"
    cases["currency_r1_when_latest"]["replay_later_list"] = {
        "published": "currency/published_v2.json", "prior": "currency/published_v1.json", "outcome": "IDENTICAL"}

    out["cases.json"] = pretty({"profile_ref": PROFILE_REF, "cases": cases})
    return out


def _flat_journal():
    j = Journal("flatness_signed")
    j.signed("flat")
    j.publish(PROFILE_REF)
    return j


def sanity(out):
    # The ACs, restated by hand.
    c = json.loads(out["cases.json"])["cases"]
    st = lambda n: [(r["rule"], r["status"], r["reason"]) for r in c[n]["expect"]["interpretation_rules"]]
    assert PROFILE_DIGEST == "sha256:287a04d6c384768b1e44c85b3c4ae851912d700b723dcd28b72e395065ec84dc"
    assert st("flatness_signed") == [(FLAT["key"], "signed_off", None), (PROFILE_ID, "provisional", "no_signoff_path")]
    assert c["flatness_signed"]["expect"]["interpretation"] == "provisional"
    assert c["flatness_signed"]["expect"]["interpretation_register"] == sha(out["flatness_signed/register.json"])
    assert [s for _, s, _ in st("position_mmc_mixed")] == ["signed_off"] * 3 + ["provisional"]
    assert len(st("position_mmc_mixed")) == 4
    assert st("tampered_row")[0] == (FLAT["key"], "provisional", "not_in_register")
    assert content_digest(TAMPERED) != content_digest(FLAT)
    for n in ("forged_register", "foreign_roster", "currency_r1_stale", "currency_rewritten_list"):
        assert all(r[1:] == ("provisional", "unverified_register") for r in st(n)), n
    assert c["forged_register"]["expect"]["verification"] == "register_mismatch"
    assert c["foreign_roster"]["expect"]["verification"] == "foreign_roster"
    assert c["currency_r1_stale"]["expect"]["verification"] == "stale_register"
    assert c["currency_rewritten_list"]["expect"]["verification"] == "published_unreadable"
    assert c["draft_row"]["expect"] == {"error": "rule_not_admitted{%s}" % POS["key"]}
    for n in ("no_rule_list", "primary_absent", "missing_profile"):
        assert c[n]["expect"] == {"error": "incomplete_result{interpretation_rules}"}, n
    assert all(r[1:] == ("provisional", "no_register") for r in st("no_register"))
    assert c["no_register"]["expect"]["interpretation_register"] == "none"
    assert st("currency_r1_when_latest")[0] == (FLAT["key"], "provisional", "awaiting_review")
    assert st("currency_r1_when_latest")[1] == (PROFILE_ID, "provisional", "not_in_register")
    assert st("currency_r2_latest")[0] == (FLAT["key"], "signed_off", None)
    # Never signed_off without a verified register.
    for n, case in c.items():
        e = case["expect"]
        if "interpretation_rules" in e and e["verification"] != "verified":
            assert all(r["status"] == "provisional" for r in e["interpretation_rules"]), n


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
