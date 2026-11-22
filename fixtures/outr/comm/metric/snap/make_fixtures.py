"""Fixtures for spec node OUTR.COMM.METRIC.SNAP (metric snapshot store and quarterly values).

Independent reference generator (stdlib only). It writes synthetic snapshot stores (`q1`,
`tampered`, `gap`) with small JSON raw responses, computes all SHA-256 digests and RFC 8785
canonical JSON records in Python (never using the Rust crate), and produces expected quarterly
JSON and Markdown files for `q1` and `gap`. All URLs use `example.org` and all counts are
fictional. Fixture content is CC-BY-4.0.

    python fixtures/outr/comm/metric/snap/make_fixtures.py          # (re)write the fixtures
    python fixtures/outr/comm/metric/snap/make_fixtures.py --check  # fail if any file differs

Layout:
    q1/
        defs.toml                                   stars (last) and downloads (sum)
        snapshots/stars/2026-12-31T23-59-59Z.*      2026Q4 snapshot (outside 2027Q1; ignored)
        snapshots/stars/2027-01-15T12-00-00Z.*      value 120
        snapshots/stars/2027-03-30T12-00-00Z.*      value 180 -> last = 180 (count 2)
        snapshots/downloads/2027-02-01T00-00-00Z.*  value 40
        snapshots/downloads/2027-03-15T00-00-00Z.*  value 60  -> sum = 100 (count 2)
    tampered/
        defs.toml                                   stars (last)
        snapshots/stars/2027-01-15T12-00-00Z.json   record with original sha256 for value 120
        snapshots/stars/2027-01-15T12-00-00Z.raw    raw response edited after collection (999)
    gap/
        defs.toml                                   stars, downloads, forks
        snapshots/stars/2027-01-15T12-00-00Z.*      value 120
        snapshots/downloads/2027-02-01T00-00-00Z.*  value 40
        snapshots/forks/2026-12-20T00-00-00Z.*      2026Q4 only -> 2027Q1 is unavailable (no_snapshot)
    expected/
        q1-2027Q1.json                              expected canonical JSON for q1/ 2027Q1
        q1-2027Q1.md                                expected Markdown summary for q1/ 2027Q1
        gap-2027Q1.json                             expected canonical JSON for gap/ 2027Q1
        gap-2027Q1.md                               expected Markdown summary for gap/ 2027Q1
"""

import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def sha256_wire(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canon_json(obj) -> bytes:
    # For ASCII strings, integers <= 2^53 - 1, booleans, arrays and dicts, sorted compact JSON
    # matches RFC 8785 (JCS) byte for byte.
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def metric_def_toml(metrics) -> bytes:
    lines = ["# Synthetic metric definitions for OUTR.COMM.METRIC.SNAP fixtures (CC-BY-4.0)."]
    for m in metrics:
        lines.append("")
        lines.append("[[metric]]")
        lines.append(f'id = "{m["id"]}"')
        lines.append(f'title = "{m["title"]}"')
        lines.append(f'source = "{m["source"]}"')
        lines.append(f'method = "{m["method"]}"')
        if "selector" in m:
            lines.append(f'selector = "{m["selector"]}"')
        lines.append(f'unit = "{m["unit"]}"')
        lines.append(f'aggregation = "{m["aggregation"]}"')
        biases_toml = ", ".join(json.dumps(b) for b in m["biases"])
        lines.append(f"biases = [{biases_toml}]")
    return ("\n".join(lines) + "\n").encode("utf-8")


def definition_digest(metric) -> str:
    return sha256_wire(canon_json(metric))


def stamp_stem(collected_at: str) -> str:
    return collected_at.replace(":", "-")


def make_snapshot(metric, collected_at: str, source_url: str, value: int, raw_bytes: bytes):
    stem = stamp_stem(collected_at)
    raw_file = f"{stem}.raw"
    record = {
        "collected_at": collected_at,
        "definition_sha256": definition_digest(metric),
        "metric_id": metric["id"],
        "raw_file": raw_file,
        "raw_sha256": sha256_wire(raw_bytes),
        "source_url": source_url,
        "unit": metric["unit"],
        "value": value,
    }
    record_bytes = canon_json(record)
    return stem, record_bytes, sha256_wire(record_bytes)


def render_markdown(quarter: str, defs_sha: str, metric_rows) -> bytes:
    lines = [
        f"# Quarterly Metric Values: {quarter}",
        "",
        f"- Quarter: `{quarter}`",
        f"- Definitions SHA-256: `{defs_sha}`",
        "",
        "| Metric | Value | Unit | Aggregation | Snapshots | Status | Flags |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for row in metric_rows:
        if row["status"] == "available":
            flags = "`definition_changed`" if row["definition_changed"] else "none"
            lines.append(
                f'| `{row["id"]}` | {row["value"]} | {row["unit"]} '
                f'| `{row["aggregation"]}` | {row["snapshot_count"]} | `available` | {flags} |'
            )
        else:
            lines.append(
                f'| `{row["id"]}` | unavailable (`{row["reason"]}`) | {row["unit"]} '
                f'| `{row["aggregation"]}` | 0 | `unavailable` | none |'
            )
    return ("\n".join(lines) + "\n").encode("utf-8")


STARS = {
    "id": "stars",
    "title": "Repository stars",
    "source": "repo-api",
    "method": "http_json",
    "selector": "/stargazers_count",
    "unit": "stars",
    "aggregation": "last",
    "biases": ["Stars are bookmarks, not evidence of active use."],
}

DOWNLOADS = {
    "id": "downloads",
    "title": "Release downloads per period",
    "source": "releases-api",
    "method": "http_json",
    "selector": "/downloads",
    "unit": "downloads",
    "aggregation": "sum",
    "biases": ["Automated CI downloads are not excluded."],
}

FORKS = {
    "id": "forks",
    "title": "Repository forks",
    "source": "repo-api",
    "method": "http_json",
    "selector": "/forks_count",
    "unit": "forks",
    "aggregation": "last",
    "biases": ["Forks created for one pull request are counted."],
}


def build():
    files = {}

    # ---------------- q1 (AC-1, AC-5) ----------------
    q1_defs = metric_def_toml([STARS, DOWNLOADS])
    files["q1/defs.toml"] = q1_defs

    stars_q4_raw = b'{"stargazers_count":99}\n'
    stem, rec, _ = make_snapshot(
        STARS, "2026-12-31T23:59:59Z", "https://api.example.org/repos/demo", 99, stars_q4_raw
    )
    files[f"q1/snapshots/stars/{stem}.json"] = rec
    files[f"q1/snapshots/stars/{stem}.raw"] = stars_q4_raw

    stars_1_raw = b'{"stargazers_count":120}\n'
    stem, rec, stars_1_sha = make_snapshot(
        STARS, "2027-01-15T12:00:00Z", "https://api.example.org/repos/demo", 120, stars_1_raw
    )
    files[f"q1/snapshots/stars/{stem}.json"] = rec
    files[f"q1/snapshots/stars/{stem}.raw"] = stars_1_raw

    stars_2_raw = b'{"stargazers_count":180}\n'
    stem, rec, stars_2_sha = make_snapshot(
        STARS, "2027-03-30T12:00:00Z", "https://api.example.org/repos/demo", 180, stars_2_raw
    )
    files[f"q1/snapshots/stars/{stem}.json"] = rec
    files[f"q1/snapshots/stars/{stem}.raw"] = stars_2_raw

    dl_1_raw = b'{"downloads":40}\n'
    stem, rec, dl_1_sha = make_snapshot(
        DOWNLOADS, "2027-02-01T00:00:00Z", "https://api.example.org/releases/latest", 40, dl_1_raw
    )
    files[f"q1/snapshots/downloads/{stem}.json"] = rec
    files[f"q1/snapshots/downloads/{stem}.raw"] = dl_1_raw

    dl_2_raw = b'{"downloads":60}\n'
    stem, rec, dl_2_sha = make_snapshot(
        DOWNLOADS, "2027-03-15T00:00:00Z", "https://api.example.org/releases/latest", 60, dl_2_raw
    )
    files[f"q1/snapshots/downloads/{stem}.json"] = rec
    files[f"q1/snapshots/downloads/{stem}.raw"] = dl_2_raw

    q1_metrics = [
        {
            "aggregation": "last",
            "definition_changed": False,
            "definition_sha256": definition_digest(STARS),
            "id": "stars",
            "snapshot_count": 2,
            "snapshot_digests": [stars_1_sha, stars_2_sha],
            "status": "available",
            "unit": "stars",
            "value": 180,
        },
        {
            "aggregation": "sum",
            "definition_changed": False,
            "definition_sha256": definition_digest(DOWNLOADS),
            "id": "downloads",
            "snapshot_count": 2,
            "snapshot_digests": [dl_1_sha, dl_2_sha],
            "status": "available",
            "unit": "downloads",
            "value": 100,
        },
    ]
    q1_report = {
        "definitions_sha256": sha256_wire(q1_defs),
        "metrics": q1_metrics,
        "quarter": "2027Q1",
        "schema": "metric-quarter@1",
        "warnings": [],
    }
    files["expected/q1-2027Q1.json"] = canon_json(q1_report)
    files["expected/q1-2027Q1.md"] = render_markdown("2027Q1", sha256_wire(q1_defs), q1_metrics)

    # ---------------- tampered (AC-2) ----------------
    tampered_defs = metric_def_toml([STARS])
    files["tampered/defs.toml"] = tampered_defs
    stem, rec, _ = make_snapshot(
        STARS, "2027-01-15T12:00:00Z", "https://api.example.org/repos/demo", 120, stars_1_raw
    )
    files[f"tampered/snapshots/stars/{stem}.json"] = rec
    # Raw response edited after collection: SHA-256 and extracted count no longer match.
    files[f"tampered/snapshots/stars/{stem}.raw"] = b'{"stargazers_count":999}\n'

    # ---------------- gap (AC-3) ----------------
    gap_defs = metric_def_toml([STARS, DOWNLOADS, FORKS])
    files["gap/defs.toml"] = gap_defs

    stem, rec, gap_stars_sha = make_snapshot(
        STARS, "2027-01-15T12:00:00Z", "https://api.example.org/repos/demo", 120, stars_1_raw
    )
    files[f"gap/snapshots/stars/{stem}.json"] = rec
    files[f"gap/snapshots/stars/{stem}.raw"] = stars_1_raw

    stem, rec, gap_dl_sha = make_snapshot(
        DOWNLOADS, "2027-02-01T00:00:00Z", "https://api.example.org/releases/latest", 40, dl_1_raw
    )
    files[f"gap/snapshots/downloads/{stem}.json"] = rec
    files[f"gap/snapshots/downloads/{stem}.raw"] = dl_1_raw

    forks_q4_raw = b'{"forks_count":15}\n'
    stem, rec, _ = make_snapshot(
        FORKS, "2026-12-20T00:00:00Z", "https://api.example.org/repos/demo", 15, forks_q4_raw
    )
    files[f"gap/snapshots/forks/{stem}.json"] = rec
    files[f"gap/snapshots/forks/{stem}.raw"] = forks_q4_raw

    gap_metrics = [
        {
            "aggregation": "last",
            "definition_changed": False,
            "definition_sha256": definition_digest(STARS),
            "id": "stars",
            "snapshot_count": 1,
            "snapshot_digests": [gap_stars_sha],
            "status": "available",
            "unit": "stars",
            "value": 120,
        },
        {
            "aggregation": "sum",
            "definition_changed": False,
            "definition_sha256": definition_digest(DOWNLOADS),
            "id": "downloads",
            "snapshot_count": 1,
            "snapshot_digests": [gap_dl_sha],
            "status": "available",
            "unit": "downloads",
            "value": 40,
        },
        {
            "aggregation": "last",
            "definition_changed": False,
            "definition_sha256": definition_digest(FORKS),
            "id": "forks",
            "reason": "no_snapshot",
            "snapshot_count": 0,
            "snapshot_digests": [],
            "status": "unavailable",
            "unit": "forks",
        },
    ]
    gap_report = {
        "definitions_sha256": sha256_wire(gap_defs),
        "metrics": gap_metrics,
        "quarter": "2027Q1",
        "schema": "metric-quarter@1",
        "warnings": [],
    }
    files["expected/gap-2027Q1.json"] = canon_json(gap_report)
    files["expected/gap-2027Q1.md"] = render_markdown("2027Q1", sha256_wire(gap_defs), gap_metrics)

    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    bad = []
    for rel, data in sorted(files.items()):
        path = os.path.join(HERE, *rel.split("/"))
        if check:
            try:
                with open(path, "rb") as f:
                    if f.read() != data:
                        bad.append(rel)
            except OSError:
                bad.append(rel)
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(data)
    if bad:
        for rel in bad:
            print(f"stale or missing: {rel}", file=sys.stderr)
        print("run: python fixtures/outr/comm/metric/snap/make_fixtures.py", file=sys.stderr)
        return 1
    print(f"{len(files)} fixture files {'match' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
