"""Independent reference for the report fixtures (IO.REG.ENV.REPORT, AC-1).

Stdlib only; shares no code with the Rust crate. It applies the leaf's rules by hand:
entity counts summed per type name and keyed in name order; warnings and dropped items
listed in source order with exact totals; source text cut to 256 Unicode code points
with a truncation flag; export `lossy` names deduplicated and sorted, with the kind of
every dropped item added. It then writes RFC 8785 canonical JSON with its own small
serialiser (64-bit integers as decimal strings, the fid-canon project rule; line and
column as JSON integers; absent optional values as null).

The inputs below must match `crates/fid-io-reg/tests/report.rs` (`basic_import`,
`basic_export`).

Run from the repository root:
    python fixtures/io/reg/env/report/make_fixtures.py          # write
    python fixtures/io/reg/env/report/make_fixtures.py --check  # compare only
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
MAX_DETAIL = 256


class U64(int):
    """A 64-bit integer, written as a decimal string."""


def jcs(v) -> str:
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, U64):
        return '"' + str(int(v)) + '"'
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        out = ['"']
        short = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\t": "\\t", "\n": "\\n", "\f": "\\f", "\r": "\\r"}
        for ch in v:
            if ch in short:
                out.append(short[ch])
            elif ord(ch) < 0x20:
                out.append("\\u%04x" % ord(ch))
            else:
                out.append(ch)
        out.append('"')
        return "".join(out)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def pos(byte, line=None, col=None):
    return {"byte": U64(byte), "line": line, "col": col}


def text(s):
    return {"text": s[:MAX_DETAIL], "truncated": len(s) > MAX_DETAIL}


# ---- AC-1: 3 point sets, 2 meshes, 4 warnings, 1 dropped item ----------------------
COUNTS = ["point_set", "mesh", "point_set", "mesh", "point_set"]  # one entity each
WARNINGS = [
    ("unit_assumed", None, ""),
    ("nan_coordinate", pos(120, 4, 7), "x=NaN"),
    ("unknown_header", pos(8), 'evil‮txt.exe\nnext\tline "q" \\ \x01\x7f'),
    ("long_comment", pos(300, 9, 1), "€" * 200 + "\U0001F600" * 100),
]
DROPPED = [("polyline", pos(512, 20, 3), "unsupported_entity", "POLYLINE 7 vertices")]


def basic_import():
    counts = {}
    for ty in COUNTS:
        counts[ty] = counts.get(ty, 0) + 1
    return {
        "format": "fid-test-points",
        "source": "root://in/ok.ftp",
        "dry_run": False,
        "entity_counts": {k: U64(n) for k, n in counts.items()},
        "warnings": [{"code": c, "pos": p, "detail": text(d)} for c, p, d in WARNINGS],
        "warnings_total": U64(len(WARNINGS)),
        "dropped": [{"kind": k, "pos": p, "reason": r, "detail": text(d)} for k, p, r, d in DROPPED],
        "dropped_total": U64(len(DROPPED)),
    }


# ---- Export: lossy names, a folded dropped kind, one warning, dry run ---------------
LOSSY = ["mesh.normals", "color", "color"]
EXPORT_DROPPED_KINDS = ["polyline"]


def basic_export():
    return {
        "format": "fid-test-points",
        "target": "root://out/a.ftp",
        "sha256": "sha256:" + "ab" * 32,
        "bytes": U64(1234),
        "dry_run": True,
        "lossy": sorted(set(LOSSY) | set(EXPORT_DROPPED_KINDS)),
        "warnings": [{"code": "precision_reduced", "pos": None, "detail": text("")}],
        "warnings_total": U64(1),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, value in [("basic.json", basic_import()), ("basic_export.json", basic_export())]:
        data = jcs(value).encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}")
                bad = 1
        else:
            path.write_bytes(data)
    return bad


if __name__ == "__main__":
    sys.exit(main())
