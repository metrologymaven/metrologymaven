#!/usr/bin/env python3
"""Generate the RPT.CATALOG.CHECK cases with Python's standard library only.

An independent reference: nothing here is produced by running the Rust crate `fid-catalog`.
The expected statuses come from this script's own reading of the leaf's rules, and the expected
content digests from Python's json and hashlib.

Output (CC-BY-4.0 project data), next to this script:

  check_cases.json  three synthetic registered projects, their catalog entries (typed columns,
                    tags and an initial entry status), each project's source journal (journal
                    position -> entry hash and the record IDs that entry carries), and the
                    scenarios: edits to the catalog or the sources, an optional project filter,
                    and the expected entry statuses, per-project counts, stale list and content
                    digest after the check.

Rules (spec/rpt/catalog/check/NODE.md):
  - an entry is "ok" when its project can be opened, the journal entry at the stored position
    exists, carries the record, and its hash equals the stored journal entry hash;
  - "stale" when the project can be opened but the journal entry is missing, does not carry the
    record, or its hash differs (REQ-002);
  - "unavailable" when the project cannot be opened, or that one journal entry cannot be read;
  - a project filter checks only that project; other entries keep their status;
  - the content digest (REQ-003, REQ-006) is the SHA-256 of the RFC 8785 canonical JSON array of
    every entry sorted by (project ID, record ID) bytewise, each entry an object of its columns
    without entry_status, with its tags as a sorted array, journal_seq as a decimal string and
    the SI value as "0x" + 16 lowercase hex digits of its IEEE 754 bits (fid-canon's number rules,
    CORE.JRN.CANON). The statuses never change the digest; altering a stored hash does.

The canonical form: json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False) equals
RFC 8785 for this data (ASCII keys; strings escape only '"', '\\' and characters below U+0020, as
both writers do; no JSON numbers appear).

Run:   python fixtures/rpt/catalog/check/make_cases.py
Check: python fixtures/rpt/catalog/check/make_cases.py --check
"""

import copy
import hashlib
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "check_cases.json"

P1 = "01936f3a-0000-7000-8000-000000000001"
P2 = "01936f3a-0000-7000-8000-000000000002"
P3 = "01936f3a-0000-7000-8000-000000000003"


def h(label):
    """A synthetic journal entry hash: sha256 of a label (never a real journal)."""
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def entry(project, record, seq, **kw):
    row = {
        "project_id": project,
        "record_id": record,
        "part": "BRACKET-100",
        "revision": "C",
        "plan": "plan-main",
        "run": "run-1",
        "stage": "final",
        "characteristic": "D1 diameter",
        "timestamp": "2026-11-20T08:00:00Z",
        "value": {"literal": "12.004", "unit": "mm", "si": "0.012004"},
        "decision_outcome": "CONFORMS",
        "rule_id": "simple-acceptance",
        "evaluation_basis": "FEATURES",
        "uec_class": "UEC-2",
        "claimed": False,
        "declared_input": False,
        "uncharacterized_input": False,
        "interpretation": "signed_off",
        "record_status": "active",
        "journal_seq": str(seq),
        "journal_entry_hash": h(f"{project}/{seq}"),
        "entry_status": "unverified",
        "tags": [],
    }
    row.update(kw)
    return row


# Registration order is deliberately not project-ID order (AC-5 also re-registers reversed).
PROJECTS = [
    {"project_id": P2, "handle": "line-b"},
    {"project_id": P1, "handle": "line-a"},
    {"project_id": P3, "handle": "line-c"},
]

# Insertion order is deliberately not (project ID, record ID) order.
ENTRIES = [
    entry(P2, "r-020", 4, characteristic="Flatness \u0001 top", tags=["lot-7", "audit"]),
    entry(P1, "r-002", 2, value={"literal": "-0.0", "unit": "mm", "si": "-0.0"}, entry_status="ok"),
    entry(P1, "r-001", 2, part="Bracket é \"q\"\\", revision=None, tags=["b", "a", "Z"]),
    entry(P3, "r-100", 1, value={"literal": "5e-5", "unit": "m", "si": "5e-5"}, entry_status="stale"),
    entry(P1, "r-010", 5, uec_class="UEC-0", value=None, decision_outcome="NOT_DECIDABLE", rule_id=None,
          interpretation="provisional"),
    entry(P2, "r-021", 6, uec_class="UEC-1", claimed=True, decision_outcome="INDETERMINATE",
          evaluation_basis="DIRECT_READING", record_status="voided"),
    entry(P3, "r-101", 3, value={"literal": "1.5", "unit": "deg", "si": "0.02617993877991494"},
          uncharacterized_input=True, uec_class="UEC-1", declared_input=True),
    entry(P2, "r-022", 6, record_status="superseded", timestamp="2026-11-21T09:30:15.250Z"),
    entry(P1, "r-003", 9, decision_outcome="NONCONFORMS", stage="first-off", entry_status="unavailable"),
    entry(P3, "r-102", 3, uec_class="UEC-3"),
]


def base_sources():
    """Each project's journal: every position an entry names, carrying exactly those records."""
    src = {}
    for e in ENTRIES:
        j = src.setdefault(e["project_id"], {})
        pos = j.setdefault(e["journal_seq"], {"hash": h(f"{e['project_id']}/{e['journal_seq']}"), "records": []})
        pos["records"].append(e["record_id"])
    # An entry the catalog does not name, so lookups must go by position.
    src[P1]["7"] = {"hash": h("p1/7"), "records": ["r-999"]}
    out = {}
    for p in sorted(src):
        out[p] = [
            {"seq": s, "hash": v["hash"], "records": sorted(v["records"]), "unreadable": False}
            for s, v in sorted(src[p].items(), key=lambda kv: int(kv[0]))
        ]
    return out


def bits(text):
    v = float(text)
    if v == 0.0:
        v = 0.0  # one stored form for zero
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", v))[0]


def digest(entries):
    rows = []
    for e in sorted(entries, key=lambda e: (e["project_id"].encode("utf-8"), e["record_id"].encode("utf-8"))):
        o = {k: v for k, v in e.items() if k != "entry_status"}
        o["tags"] = sorted(e["tags"], key=lambda t: t.encode("utf-8"))
        if e["value"] is not None:
            o["value"] = {"literal": e["value"]["literal"], "unit": e["value"]["unit"], "si": bits(e["value"]["si"])}
        rows.append(o)
    text = json.dumps(rows, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def judge(sources, e):
    journal = sources.get(e["project_id"])
    if journal is None:
        return "unavailable"
    pos = next((p for p in journal if p["seq"] == e["journal_seq"]), None)
    if pos is None:
        return "stale"
    if pos["unreadable"]:
        return "unavailable"
    if e["record_id"] not in pos["records"]:
        return "stale"
    return "ok" if pos["hash"] == e["journal_entry_hash"] else "stale"


def expect(entries, sources, flt):
    statuses = {}
    per = []
    stale = []
    for p in sorted({e["project_id"] for e in entries} | {x["project_id"] for x in PROJECTS}):
        checked = flt is None or flt == p
        counts = {"project_id": p, "ok": 0, "stale": 0, "unavailable": 0}
        for e in sorted((e for e in entries if e["project_id"] == p), key=lambda e: e["record_id"].encode("utf-8")):
            key = f"{p}/{e['record_id']}"
            if not checked:
                statuses[key] = e["entry_status"]
                continue
            s = judge(sources, e)
            statuses[key] = s
            counts[s] += 1
            if s == "stale":
                stale.append([p, e["record_id"]])
        if checked:
            per.append(counts)
    return {"statuses": statuses, "per_project": per, "stale": stale, "digest": digest(entries)}


def scenario(name, ac, catalog_edits=(), remove=(), drop=(), unreadable=(), rehash=(), flt=None):
    entries = copy.deepcopy(ENTRIES)
    for p, r, new in catalog_edits:
        [e] = [e for e in entries if e["project_id"] == p and e["record_id"] == r]
        e["journal_entry_hash"] = new
    sources = base_sources()
    for p in remove:
        del sources[p]
    for p, s, r in drop:
        [pos] = [x for x in sources[p] if x["seq"] == s]
        pos["records"].remove(r)
    for p, s in unreadable:
        [pos] = [x for x in sources[p] if x["seq"] == s]
        pos["unreadable"] = True
    for p, s, new in rehash:
        [pos] = [x for x in sources[p] if x["seq"] == s]
        pos["hash"] = new
    return {
        "name": name,
        "ac": ac,
        "catalog_edits": [{"project_id": p, "record_id": r, "journal_entry_hash": n} for p, r, n in catalog_edits],
        "remove_projects": list(remove),
        "drop_records": [{"project_id": p, "seq": s, "record_id": r} for p, s, r in drop],
        "unreadable": [{"project_id": p, "seq": s} for p, s in unreadable],
        "rehash": [{"project_id": p, "seq": s, "hash": n} for p, s, n in rehash],
        "filter": flt,
        "expect": expect(entries, sources, flt),
    }


# Paging (more entries than one catalog page of 10 000): project P1 holds BULK_COUNT entries
# "r-00000" .. "r-10000" at journal positions 1 .. BULK_COUNT, all sharing one hash, and P2 one
# entry; the Rust test generates the same rows from these parameters.
BULK_COUNT = 10_001
BULK_HASH = h("bulk")


def bulk():
    rows = [entry(P1, f"r-{i:05d}", i + 1, journal_entry_hash=BULK_HASH, value=None, uec_class="UEC-0",
                  decision_outcome="NOT_DECIDABLE", rule_id=None, revision=None) for i in range(BULK_COUNT)]
    rows.append(entry(P2, "r-020", 4))
    return {
        "count": BULK_COUNT,
        "hash": BULK_HASH,
        "template": entry(P1, "r-00000", 1, journal_entry_hash=BULK_HASH, value=None, uec_class="UEC-0",
                          decision_outcome="NOT_DECIDABLE", rule_id=None, revision=None),
        "other": entry(P2, "r-020", 4),
        "digest": digest(rows),
    }


def build():
    scenarios = [
        scenario("all_ok", "AC-1"),
        scenario("altered_catalog_hash", "AC-2", catalog_edits=[(P2, "r-021", h("tampered"))]),
        scenario("project_removed", "AC-3", remove=[P3]),
        scenario("journal_rewritten", "REQ-002", rehash=[(P1, "2", h("rewritten"))]),
        scenario("record_not_at_position", "REQ-002", drop=[(P2, "6", "r-022")]),
        scenario("entry_unreadable", "UNAVAILABLE", unreadable=[(P3, "3")]),
        scenario("filter_one_project", "REQ-001", catalog_edits=[(P1, "r-003", h("t1")), (P2, "r-020", h("t2"))],
                 flt=P2),
    ]
    return {
        "projects": PROJECTS,
        "entries": ENTRIES,
        "sources": base_sources(),
        "digest": digest(ENTRIES),
        "empty_digest": digest([]),
        "scenarios": scenarios,
        "bulk": bulk(),
    }


def render(data):
    return json.dumps(data, indent=2, ensure_ascii=True, sort_keys=False) + "\n"


def main():
    text = render(build())
    if "--check" in sys.argv[1:]:
        if OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} is out of date; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print("check_cases.json up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
