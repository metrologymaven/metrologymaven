#!/usr/bin/env python3
"""Synthetic fixtures and independent reference outputs for MIG.SRC.REG (project-authored, CC0-1.0).

Every product here is fictional (DemoCMM, DemoScan), so no fixture makes a claim about a real
incumbent; every note is the project's own wording. Nothing here runs the Rust code.

Writes, next to itself:

- register-ok.json: a valid mig-sources@1 register (policy D-033-A, five entries, inline
  mig-review@1 records).
- admitted-noreview.json: src.demoscan-points-csv is ADMITTED without a review record
  (SRC-REVIEW).
- native-admitted.json: the native program format src.democmm-program-native is ADMITTED,
  fully reviewed, under D-033-A (SRC-POLICY).
- dup.json: a second DemoCMM results/qif entry (product in other case) whose version range
  overlaps the first (SRC-DUP).
- attest-missing.json: one review record has no_copied_text_attestation false (SRC-ATTEST).
- schema-bad.json: a product name holding a Markdown table bar (SRC-SCHEMA).
- vocab-claim.json: an export note with an unsupported conformance claim (SRC-VOCAB under
  the outreach claim rules).
- expected/list-not-admitted.json: RFC 8785 canonical JSON of the migrate.list_sources@1
  result for register-ok.json with status NOT_ADMITTED.
- expected/sources.md: the sources page for register-ok.json (format md, no filter).

The listing and page rules restated from the leaf and crates/fid-mig/src/src/reg.rs: entries
sorted by ID; result {register_version, register_digest, policy, filter, entries}; an entry
lists id, product, kind, format_id, versions, access_route, status and export_route when
present; register_digest is sha256 of the register's canonical JSON. The page layout is
pinned in render_md below (IDs, formats and version ranges in code spans, so `*` stays literal).

Run:  python fixtures/mig/src/reg/make_fixtures.py          (write)
      python fixtures/mig/src/reg/make_fixtures.py --check  (compare, exit 1 on drift)
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def review(entry, **over):
    r = {
        "schema": "mig-review@1",
        "entry": entry,
        "basis": "The user exports the data to an open format whose public specification we implement.",
        "reviewer": "maintainer-a",
        "date": "2026-11-20",
        "clean_room_statement": "No code, text or assets of the product were used; only the open format specification.",
        "eula_note": "Reading a user-made open export needs no licence term of the product.",
        "no_copied_text_attestation": True,
    }
    r.update(over)
    return r


def register_ok():
    return {
        "schema": "mig-sources@1",
        "version": "0.3.0",
        "policy": "D-033-A",
        "entries": [
            {
                "id": "src.demoscan-points-csv",
                "product": "DemoScan",
                "kind": "points",
                "format_id": "points-csv",
                "versions": "2..*",
                "access_route": "PUBLIC_SPEC",
                "status": "ADMITTED",
                "review": review("src.demoscan-points-csv"),
            },
            {
                "id": "src.democmm-program-native",
                "product": "DemoCMM",
                "kind": "program",
                "format_id": "native:democmm-prog",
                "versions": "3..5.2",
                "access_route": "NONE",
                "status": "NOT_ADMITTED",
                "export_route": {
                    "format": "dmis",
                    "note": "Save the program as DMIS from the DemoCMM program editor, then import the DMIS file.",
                },
            },
            {
                "id": "src.democmm-results-qif",
                "product": "DemoCMM",
                "kind": "results",
                "format_id": "qif",
                "versions": "*..*",
                "access_route": "OPEN_EXPORT",
                "status": "OPEN_EXPORT_ONLY",
                "review": review("src.democmm-results-qif"),
            },
            {
                "id": "src.democmm-project-native",
                "product": "DemoCMM",
                "kind": "project",
                "format_id": "native:democmm-proj",
                "versions": "*..*",
                "access_route": "NONE",
                "status": "PENDING_REVIEW",
                "export_route": {
                    "format": "none",
                    "note": "No open export exists yet; keep the project in DemoCMM and bring its results over as QIF.",
                },
                "review": review("src.democmm-project-native", basis="", reviewer="", date=""),
            },
            {
                "id": "src.demoscan-form-native",
                "product": "DemoScan",
                "kind": "form",
                "format_id": "native:demoscan-form",
                "versions": "2020..2024",
                "access_route": "NONE",
                "status": "NOT_ADMITTED",
                "export_route": {
                    "format": "results-csv",
                    "note": "Export the report values as CSV and rebuild the form from the project's own templates.",
                },
            },
        ],
    }


def by_id(reg, eid):
    return next(e for e in reg["entries"] if e["id"] == eid)


def variants():
    out = {"register-ok.json": register_ok()}
    r = register_ok()
    del by_id(r, "src.demoscan-points-csv")["review"]
    out["admitted-noreview.json"] = r
    r = register_ok()
    e = by_id(r, "src.democmm-program-native")
    e["status"] = "ADMITTED"
    e["access_route"] = "PUBLIC_SPEC"
    del e["export_route"]
    e["review"] = review("src.democmm-program-native")
    out["native-admitted.json"] = r
    r = register_ok()
    e = copy.deepcopy(by_id(r, "src.democmm-results-qif"))
    e["id"] = "src.democmm-results-qif-2"
    e["product"] = "democmm"
    e["versions"] = "2..3"
    e["review"]["entry"] = e["id"]
    r["entries"].append(e)
    out["dup.json"] = r
    r = register_ok()
    by_id(r, "src.democmm-results-qif")["review"]["no_copied_text_attestation"] = False
    out["attest-missing.json"] = r
    r = register_ok()
    by_id(r, "src.democmm-results-qif")["product"] = "DemoCMM | Pro"
    out["schema-bad.json"] = r
    r = register_ok()
    by_id(r, "src.democmm-program-native")["export_route"]["note"] = "The DMIS export is certified for every program."
    out["vocab-claim.json"] = r
    return out


def canonical(v):
    # RFC 8785 for this data: strings, booleans, objects and arrays only; ASCII keys.
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def digest(v):
    return "sha256:" + hashlib.sha256(canonical(v)).hexdigest()


def listing(reg, kind=None, status=None):
    entries = []
    for e in sorted(reg["entries"], key=lambda e: e["id"]):
        if kind is not None and e["kind"] != kind:
            continue
        if status is not None and e["status"] != status:
            continue
        x = {k: e[k] for k in ("id", "product", "kind", "format_id", "versions", "access_route", "status")}
        if "export_route" in e:
            x["export_route"] = dict(e["export_route"])
        entries.append(x)
    flt = {}
    if kind is not None:
        flt["kind"] = kind
    if status is not None:
        flt["status"] = status
    return {
        "register_version": reg["version"],
        "register_digest": digest(reg),
        "policy": reg["policy"],
        "filter": flt,
        "entries": entries,
    }


def render_md(lst):
    v, d = lst["register_version"], lst["register_digest"]
    lines = [
        "<!-- Generated by fid migrate list-sources (migrate.list_sources@1) from mig-sources@1 version %s (%s). Do not edit. -->" % (v, d),
        "# Incumbent sources",
        "",
        "Generated by `fid migrate list-sources --format md` from register mig-sources@1 version %s, digest `%s`. Do not edit." % (v, d),
        "",
        "Policy: %s." % lst["policy"],
    ]
    f = lst["filter"]
    if f:
        lines.append("Filter: kind %s, status %s." % (f.get("kind", "any"), f.get("status", "any")))
    lines.append("")
    if not lst["entries"]:
        lines.append("No entries.")
        return "\n".join(lines) + "\n"
    lines.append("| ID | Product | Artefact | Format | Versions | Access route | Status | Export to |")
    lines.append("|---|---|---|---|---|---|---|---|")
    for e in lst["entries"]:
        r = e.get("export_route")
        if r is None:
            export = "-"
        elif r["format"] == "none":
            export = "none"
        else:
            export = "`%s`" % r["format"]
        lines.append(
            "| `%s` | %s | %s | `%s` | `%s` | %s | %s | %s |"
            % (e["id"], e["product"], e["kind"], e["format_id"], e["versions"], e["access_route"], e["status"], export)
        )
    routed = [e for e in lst["entries"] if "export_route" in e]
    if routed:
        lines += ["", "## Export routes", ""]
        for e in routed:
            lines.append("- `%s`: %s" % (e["id"], e["export_route"]["note"]))
    return "\n".join(lines) + "\n"


def outputs():
    out = {}
    for name, reg in variants().items():
        out[name] = json.dumps(reg, indent=2, ensure_ascii=False) + "\n"
    ok = register_ok()
    na = listing(ok, status="NOT_ADMITTED")
    assert [e["id"] for e in na["entries"]] == ["src.democmm-program-native", "src.demoscan-form-native"]
    out["expected/list-not-admitted.json"] = canonical(na).decode() + "\n"
    out["expected/sources.md"] = render_md(listing(ok))
    return out


def main():
    check = "--check" in sys.argv[1:]
    drift = False
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                print("drift: " + name)
                drift = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode())
    return 1 if drift else 0


if __name__ == "__main__":
    sys.exit(main())
