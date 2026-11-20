#!/usr/bin/env python3
"""Validate the kernel-manifest fixtures with an independent JSON Schema validator.

KRN.MAN.MANI.SCHEMA AC-3, and the schema half of the schema/Rust agreement check: every
file in fixtures/man/manifest/labels.txt must get its labelled schema outcome from
`jsonschema` (Draft 2020-12) against schemas/kernel-manifest.schema.json. The Rust half,
crates/fid-kci/tests/manifest.rs, checks the same labels against `KernelManifest::parse`.

It also checks the labels are consistent (schema-invalid implies a Rust rejection, a Rust
`ok` implies schema-valid), that every fixture has exactly one label, and that the list and
length bounds of the schema reject one item or byte past the Rust limits.

Run:  python fixtures/man/check_manifest_schema.py   (needs `pip install jsonschema`)
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

HERE = Path(__file__).parent
ROOT = HERE.parent.parent
SCHEMA = json.loads((ROOT / "schemas" / "kernel-manifest.schema.json").read_text(encoding="utf-8"))
Draft202012Validator.check_schema(SCHEMA)
VALIDATOR = Draft202012Validator(SCHEMA)


def strict_load(data):
    """UTF-8 without BOM, no duplicate keys, no NaN/Infinity, no raw control characters."""
    def pairs(items):
        keys = [k for k, _ in items]
        if len(keys) != len(set(keys)):
            raise ValueError("duplicate key")
        return dict(items)

    def no_constant(name):
        raise ValueError(name)

    text = data.decode("utf-8")
    return json.loads(text, object_pairs_hook=pairs, parse_constant=no_constant)


def schema_outcome(data):
    try:
        value = strict_load(data)
    except ValueError:  # includes UnicodeDecodeError and json.JSONDecodeError
        return "unparseable"
    return "valid" if VALIDATOR.is_valid(value) else "invalid"


def path_of(name):
    return HERE / name if name == "manifest-v1.json" else HERE / "manifest" / name


def main():
    failures = []
    labelled = set()
    for line in (HERE / "manifest" / "labels.txt").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        name, rust, _detail, schema = line.split("\t")
        if name in labelled:
            failures.append(f"{name}: labelled twice")
        labelled.add(name)
        if schema not in ("valid", "invalid", "unparseable"):
            failures.append(f"{name}: bad schema label {schema!r}")
        if schema != "valid" and rust == "ok":
            failures.append(f"{name}: labels disagree (schema {schema}, rust ok)")
        got = schema_outcome(path_of(name).read_bytes())
        if got != schema:
            failures.append(f"{name}: schema says {got}, label says {schema}")
    on_disk = {p.name for p in (HERE / "manifest").glob("*.json")} | {"manifest-v1.json"}
    for name in sorted(on_disk ^ labelled):
        failures.append(f"{name}: {'unlabelled' if name in on_disk else 'labelled but missing'}")

    # Bounds: the Rust limits (MAX_ALGORITHMS 4096, MAX_ARTIFACTS 256, MAX_TOKEN_LEN 64,
    # MAX_ID_LEN 128) are accepted, one more is refused.
    base = strict_load((HERE / "manifest-v1.json").read_bytes())

    def algs(n):
        return [{"id": f"a.b.c@{i}.0.0", "card_digest": "sha256:" + "0" * 64, "impl_digest": "sha256:" + "1" * 64}
                for i in range(n)]

    def arts(n):
        return [{"role": "native", "target": f"t{i:04d}", "sha256": "sha256:" + "2" * 64} for i in range(n)]

    long_id = "a" * 41 + "." + "b" * 41 + "." + "c" * 29 + "@4294967295.0.0"
    assert len(long_id) == 128
    cases = [
        (["core", "algorithms"], algs(4096), True),
        (["core", "algorithms"], algs(4097), False),
        (["artifacts"], arts(256), True),
        (["artifacts"], arts(257), False),
        (["core", "name"], "n" * 64, True),
        (["core", "name"], "n" * 65, False),
        (["core", "semver"], "1.0.0-" + "r" * 58, True),
        (["core", "semver"], "1.0.0-" + "r" * 59, False),
        (["artifacts"], [{"role": "native", "target": "t" * 64, "sha256": "sha256:" + "2" * 64}], True),
        (["artifacts"], [{"role": "native", "target": "t" * 65, "sha256": "sha256:" + "2" * 64}], False),
        (["core", "algorithms"], [dict(algs(1)[0], id=long_id)], True),
        (["core", "algorithms"], [dict(algs(1)[0], id="a" + long_id)], False),
        # KRN.MAN.MANI.GEN: module `kernel` is reserved for the kernel's own queries (Rust:
        # bad_value core.algorithms[].id); a neighbouring module name is an ordinary ID.
        (["core", "algorithms"], [dict(algs(1)[0], id="kernel.manifest.core_digest@1.0.0")], False),
        (["core", "algorithms"], [dict(algs(1)[0], id="kernels.manifest.core_digest@1.0.0")], True),
    ]
    for path, value, ok in cases:
        m = copy.deepcopy(base)
        target = m
        for p in path[:-1]:
            target = target[p]
        target[path[-1]] = value
        if VALIDATOR.is_valid(m) != ok:
            size = len(value) if isinstance(value, (list, str)) else value
            failures.append(f"bound {'.'.join(path)} size {size}: expected {'valid' if ok else 'invalid'}")

    if failures:
        print("kernel-manifest schema check failed:\n  " + "\n  ".join(failures))
        sys.exit(1)
    print(f"kernel-manifest schema: {len(labelled)} labelled files and {len(cases)} bounds agree")


if __name__ == "__main__":
    main()
