"""Fixtures for spec node OUTR.BENCH.RUN.RECIPE (benchmark recipe format and validator).

Independent reference generator (stdlib only). It writes two small synthetic data files, a stub
dataset catalogue per case listing their SHA-256 digests (computed here in Python, never by the
Rust code), hand-written `bench-recipe@1` recipes, and the expected canonical JSON report of the
`ok` case. Everything is fictional and project-made: no third-party data, no real benchmark
numbers. Fixture content is CC-BY-4.0.

    python fixtures/outr/bench/recipe/make_fixtures.py          # (re)write the fixtures
    python fixtures/outr/bench/recipe/make_fixtures.py --check  # fail if any file differs

Layout (each case folder has catalogue.toml and recipes/):
    data/               two synthetic files whose digests the catalogues list
    ok/                 one valid recipe per kind (AC-1)
    missing-tolerance/  a result field without `tolerance` (AC-2: field_malformed)
    bad-digest/         dataset sha256 differs from the catalogue (AC-3: dataset_digest_mismatch)
    other-program/      command source with argv[0] = "python" (AC-4: source_invalid)
    dup-id/             two files stating the same id (AC-5: duplicate_id)
    unparsable/         a recipe that is not TOML (AC-6: exit 2)
    expected/ok.json    report of `bench-recipe check --catalogue catalogue.toml recipes --json`
                        run inside ok/ (AC-1, AC-7)
"""

import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

PLANE = (
    "# Synthetic, fictional point set for OUTR.BENCH.RUN.RECIPE fixtures (CC-BY-4.0).\n"
    "x_mm,y_mm,z_mm\n"
    "0.000,0.000,0.001\n"
    "10.000,0.000,-0.002\n"
    "0.000,10.000,0.000\n"
    "10.000,10.000,0.001\n"
).encode("utf-8")

TIMING = (
    "# Synthetic, fictional workload description for OUTR.BENCH.RUN.RECIPE fixtures (CC-BY-4.0).\n"
    "step,points\n"
    "load,1000\n"
    "fit,1000\n"
).encode("utf-8")

PLANE_ID = "outr-bench-fixture-plane-v1"
TIMING_ID = "outr-bench-fixture-timing-v1"


def hexdigest(data):
    return hashlib.sha256(data).hexdigest()


def catalogue():
    out = "# Stub dataset catalogue for OUTR.BENCH.RUN.RECIPE fixtures (ECO.DATA.CAT layout).\n"
    for ds_id, name, data in [(PLANE_ID, "plane-points-v1.csv", PLANE), (TIMING_ID, "timing-trace-v1.csv", TIMING)]:
        out += (
            "\n[[dataset]]\n"
            f'id = "{ds_id}"\n'
            f'title = "Synthetic fixture file {name}"\n'
            'licence = "CC-BY-4.0"\n'
            f"size_bytes = {len(data)}\n"
            f'sha256 = "{hexdigest(data)}"\n'
        )
    return out.encode("utf-8")


def recipe(rid, kind, dataset_id, dataset_data, source, fields, profile="any", section=None, drop=()):
    lines = [
        "# Fictional benchmark recipe (fixture, CC-BY-4.0).",
        'schema = "bench-recipe@1"',
        f'id = "{rid}"',
        f'kind = "{kind}"',
        f'title = "Fixture recipe {rid}"',
        'since_release = "v0.1.0"',
        f'machine_profile = "{profile}"',
        f'dataset = {{ id = "{dataset_id}", sha256 = "sha256:{hexdigest(dataset_data)}" }}',
        "",
        "[source]",
    ]
    lines += source
    for f in fields:
        lines += ["", "[[fields]]"]
        for key in ["name", "unit", "better", "tolerance", "tolerance_kind"]:
            if (f["name"], key) not in drop:
                lines.append(f'{key} = "{f[key]}"')
    lines += ["", f"[{section or kind}]", 'note = "stub section; the kind validator of this kind owns its rules"']
    return ("\n".join(lines) + "\n").encode("utf-8")


ERR = {"name": "max_abs_error", "unit": "mm", "better": "lower", "tolerance": "0.0005", "tolerance_kind": "abs"}
RMS = {"name": "rms_error", "unit": "mm", "better": "lower", "tolerance": "0.05", "tolerance_kind": "rel"}
WALL = {"name": "wall_time", "unit": "ms", "better": "lower", "tolerance": "0.10", "tolerance_kind": "rel"}
COUNT = {"name": "points_used", "unit": "1", "better": "equal", "tolerance": "0", "tolerance_kind": "abs"}


def release_source(rid):
    return [
        'type = "release_numbers"',
        f'selector = "/results/{rid}"',
        f'journal_selector = "/journals/{rid}"',
    ]


def command_source(program, fields):
    outputs = ", ".join(f'{f["name"]} = "/fields/{f["name"]}"' for f in fields)
    return [
        'type = "command"',
        f'argv = ["{program}", "bench", "run", "--dataset", "plane-points-v1.csv"]',
        f"outputs = {{ {outputs} }}",
    ]


def build():
    files = {
        "data/plane-points-v1.csv": PLANE,
        "data/timing-trace-v1.csv": TIMING,
    }
    cases = ["ok", "missing-tolerance", "bad-digest", "other-program", "dup-id", "unparsable"]
    for case in cases:
        files[f"{case}/catalogue.toml"] = catalogue()

    ok = {
        "accuracy.toml": ("fixture.accuracy-plane", "accuracy"),
        "comparison.toml": ("fixture.comparison-plane", "comparison"),
        "fidelity.toml": ("fixture.fidelity-plane", "fidelity"),
        "performance.toml": ("fixture.performance-fit", "performance"),
    }
    files["ok/recipes/accuracy.toml"] = recipe(
        "fixture.accuracy-plane", "accuracy", PLANE_ID, PLANE, release_source("fixture.accuracy-plane"), [ERR, COUNT]
    )
    files["ok/recipes/comparison.toml"] = recipe(
        "fixture.comparison-plane", "comparison", PLANE_ID, PLANE, release_source("fixture.comparison-plane"), [RMS]
    )
    files["ok/recipes/fidelity.toml"] = recipe(
        "fixture.fidelity-plane", "fidelity", PLANE_ID, PLANE, command_source("fid", [RMS, COUNT]), [RMS, COUNT]
    )
    files["ok/recipes/performance.toml"] = recipe(
        "fixture.performance-fit",
        "performance",
        TIMING_ID,
        TIMING,
        command_source("fid-verify", [WALL]),
        [WALL],
        profile="recommended",
    )

    files["missing-tolerance/recipes/recipe.toml"] = recipe(
        "fixture.missing-tolerance",
        "accuracy",
        PLANE_ID,
        PLANE,
        release_source("fixture.missing-tolerance"),
        [ERR],
        drop={("max_abs_error", "tolerance")},
    )
    # The plane dataset ID with the timing file's digest.
    files["bad-digest/recipes/recipe.toml"] = recipe(
        "fixture.bad-digest", "accuracy", PLANE_ID, TIMING, release_source("fixture.bad-digest"), [ERR]
    )
    files["other-program/recipes/recipe.toml"] = recipe(
        "fixture.other-program", "fidelity", PLANE_ID, PLANE, command_source("python", [RMS]), [RMS]
    )
    for name in ["a.toml", "b.toml"]:
        files[f"dup-id/recipes/{name}"] = recipe(
            "fixture.same-id", "accuracy", PLANE_ID, PLANE, release_source("fixture.same-id"), [ERR]
        )
    files["unparsable/recipes/broken.toml"] = (
        b'# Not TOML: the table header is never closed (fixture, CC-BY-4.0).\n'
        b'schema = "bench-recipe@1"\n'
        b"[source\n"
    )

    report = {
        "findings": [],
        "recipes": [{"file": f"recipes/{name}", "id": rid, "kind": kind} for name, (rid, kind) in sorted(ok.items())],
        "status": "ok",
    }
    # RFC 8785 for this data: ASCII strings, arrays and objects only.
    files["expected/ok.json"] = json.dumps(report, sort_keys=True, separators=(",", ":")).encode("utf-8") + b"\n"
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
        print("run: python fixtures/outr/bench/recipe/make_fixtures.py", file=sys.stderr)
        return 1
    print(f"{len(files)} fixture files {'match' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
