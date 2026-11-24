#!/usr/bin/env python3
"""Synthetic fixtures and independent reference outputs for MIG.COVER.CORPUS (project-authored, CC0-1.0).

Every product here is fictional (DemoCMM, DemoScan) and every dataset is invented: the
"files" are never fetched, their SHA-256 values are digests of short made-up strings, and the
URLs point at example.org. No vendor file, no third-party data and no QIF schema is used
(D-038). Nothing here runs the Rust code.

Writes, next to itself:

- datasets.toml: a dataset catalogue extract in the ECO.DATA.CAT schema with six entries:
  three mirrored QIF demo files, one fetch-only QIF file, one mirrored QIF file that declares
  customer data, and one mirrored points-csv file.
- sources.json: a mig-sources@1 register extract: qif is OPEN_EXPORT_ONLY (reviewed),
  points-csv is PENDING_REVIEW.
- ok/corpus.json: three public qif entries matching the catalogue (AC-1), in file order
  c, a, b; ok/expected-digest.txt holds its corpus digest.
- ok/corpus-shuffled.json: the same entries in order b, c, a (AC-6: same digest).
- missing.json: one catalogue ID not in the catalogue (AC-2, CORPUS-UNCATALOGUED).
- public-fetchonly.json: a public entry whose catalogue entry is fetch-only (AC-3,
  CORPUS-VISIBILITY).
- pending-source.json: a points-csv corpus, whose register entry is PENDING_REVIEW (AC-4,
  CORPUS-SOURCE points-csv).
- mixed.json: two public entries and the fetch-only file as private (AC-5, passes with
  private_run: true); expected-digest-mixed.txt holds its digest.
- digest-mismatch.json: an entry whose SHA-256 differs from the catalogue (CORPUS-DIGEST).
- public-customer.json: a public entry whose catalogue entry declares customer data
  (CORPUS-VISIBILITY).
- dup-entry.json: one catalogue ID listed twice (CORPUS-DUP).
- format-mismatch.json: an entry whose format differs from the manifest's (CORPUS-SCHEMA).
- unknown-key.json: an entry with an unknown key (CORPUS-SCHEMA).
- registry-qif.json / registry-empty.json: synthetic registry manifests whose format
  catalogue registers qif and points-csv, or nothing.

The corpus digest rule restated from the leaf (REQ-005): SHA-256 of the RFC 8785 canonical
JSON of the manifest with entries sorted by catalogue ID, written as sha256:<hex>. The
manifests hold only ASCII strings, arrays and objects (no numbers), for which RFC 8785 is
JSON with keys sorted and no whitespace, so json.dumps(sort_keys=True,
separators=(",", ":")) is an independent implementation of it here.

Run:  python fixtures/mig/cover/corpus/make_fixtures.py          (write)
      python fixtures/mig/cover/corpus/make_fixtures.py --check  (compare, exit 1 on drift)
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def fake_sha(label):
    """Digest of an invented label, standing in for a file the fixtures never hold."""
    return hashlib.sha256(("mig corpus fixture file " + label).encode()).hexdigest()


# (id, licence, redistribution, contains_customer_data, file label)
DATASETS = [
    ("mig-qif-demo-a", "CC0-1.0", "mirror", False, "a"),
    ("mig-qif-demo-b", "CC0-1.0", "mirror", False, "b"),
    ("mig-qif-demo-c", "CC-BY-4.0", "mirror", False, "c"),
    ("mig-qif-fetchonly", "LicenseRef-Demo-Terms", "fetch-only", False, "fetchonly"),
    ("mig-qif-customer", "CC0-1.0", "mirror", True, "customer"),
    ("mig-csv-demo", "CC0-1.0", "mirror", False, "csv"),
]
SHA = {d[0]: fake_sha(d[4]) for d in DATASETS}


def catalogue():
    lines = [
        "# MIG.COVER.CORPUS fixture: a dataset catalogue extract (ECO.DATA.CAT schema).",
        "# Synthetic entries only; nothing here is fetched. CC0-1.0.",
        "[policy]",
        'redistributable = ["CC0-1.0", "CC-BY-4.0", "MIT", "Apache-2.0"]',
    ]
    for ident, licence, redistribution, customer, _label in DATASETS:
        lines += [
            "",
            "[[dataset]]",
            f'id = "{ident}"',
            f'title = "Synthetic migration corpus file {ident}"',
            f'source = "https://example.org/datasets/{ident}"',
            f'licence = "{licence}"',
        ]
        if licence.startswith("LicenseRef-"):
            lines.append('terms_url = "https://example.org/terms"')
        lines += [
            "size_bytes = 2048",
            f'sha256 = "{SHA[ident]}"',
            'owner = "MIG"',
            f'redistribution = "{redistribution}"',
            f'urls = ["https://example.org/files/{ident}.zip"]',
        ]
        if customer:
            lines += ["contains_customer_data = true", 'release_ref = "decisions/D-999-example-release.md"']
    return "\n".join(lines) + "\n"


def review(entry):
    return {
        "schema": "mig-review@1",
        "entry": entry,
        "basis": "The user exports the data to an open format whose public specification we implement.",
        "reviewer": "maintainer-a",
        "date": "2026-11-20",
        "clean_room_statement": "No code, text or assets of the product were used; only the open format specification.",
        "eula_note": "Reading a user-made open export needs no licence term of the product.",
        "no_copied_text_attestation": True,
    }


def sources():
    return {
        "schema": "mig-sources@1",
        "version": "0.1.0",
        "policy": "D-033-A",
        "entries": [
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
                "id": "src.demoscan-points-csv",
                "product": "DemoScan",
                "kind": "points",
                "format_id": "points-csv",
                "versions": "2..*",
                "access_route": "PUBLIC_SPEC",
                "status": "PENDING_REVIEW",
                "export_route": {
                    "format": "qif",
                    "note": "Until the review is done, export the measured points as QIF results and import those.",
                },
            },
        ],
    }


def entry(ident, visibility="public", fmt="qif", **over):
    e = {"catalogue_id": ident, "sha256": "sha256:" + SHA.get(ident, fake_sha(ident)), "format_id": fmt,
         "visibility": visibility}
    e.update(over)
    return e


def manifest(entries, fmt="qif", name="QIF results demo corpus"):
    return {"schema": "mig-corpus@1", "format_id": fmt, "name": name, "entries": entries}


def corpus_digest(m):
    m = copy.deepcopy(m)
    m["entries"] = sorted(m["entries"], key=lambda e: e["catalogue_id"])
    canonical = json.dumps(m, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return "sha256:" + hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def as_json(v):
    return json.dumps(v, indent=2, ensure_ascii=False) + "\n"


def outputs():
    a, b, c = entry("mig-qif-demo-a"), entry("mig-qif-demo-b"), entry("mig-qif-demo-c", note="Hand-made demo results file.")
    ok = manifest([c, a, b])
    shuffled = manifest([b, c, a])
    assert corpus_digest(ok) == corpus_digest(shuffled)
    mixed = manifest([a, b, entry("mig-qif-fetchonly", visibility="private")])
    bad_digest = entry("mig-qif-demo-b")
    bad_digest["sha256"] = "sha256:" + fake_sha("not b")
    out = {
        "datasets.toml": catalogue(),
        "sources.json": as_json(sources()),
        "ok/corpus.json": as_json(ok),
        "ok/corpus-shuffled.json": as_json(shuffled),
        "ok/expected-digest.txt": corpus_digest(ok) + "\n",
        "missing.json": as_json(manifest([a, entry("mig-qif-demo-z")])),
        "public-fetchonly.json": as_json(manifest([a, entry("mig-qif-fetchonly")])),
        "pending-source.json": as_json(
            manifest([entry("mig-csv-demo", fmt="points-csv")], fmt="points-csv", name="Point export demo corpus")
        ),
        "mixed.json": as_json(mixed),
        "expected-digest-mixed.txt": corpus_digest(mixed) + "\n",
        "digest-mismatch.json": as_json(manifest([a, bad_digest])),
        "public-customer.json": as_json(manifest([a, entry("mig-qif-customer")])),
        "dup-entry.json": as_json(manifest([a, b, copy.deepcopy(a)])),
        "format-mismatch.json": as_json(manifest([a, entry("mig-csv-demo", fmt="points-csv")])),
        "unknown-key.json": as_json(manifest([a, entry("mig-qif-demo-b", url="https://example.org/b")])),
        "registry-qif.json": as_json({"formats": [{"id": "points-csv"}, {"id": "qif"}]}),
        "registry-empty.json": as_json({"commands": []}),
    }
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
