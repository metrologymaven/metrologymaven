#!/usr/bin/env python3
"""Independent reference for UNC.MODEL.GRAPH.STORE fixtures (stdlib only; CC-BY-4.0 data).

Written from spec/unc/model/graph/store/NODE.md and the module documentation of fid-unc
`model_store`, never from the Rust code. Every expected value is either a SHA-256 from
hashlib, a byte length, or text labelled by hand from the spec's rules below.

TOY MANIFEST. `toy_release_manifest.json` is a kernel manifest in the KRN.MAN.MANI.SCHEMA
form whose name says what it is: `toy-test-only-not-a-release`. No real release exists yet
(KRN.MAN.MANI.GEN "Release blocked"), and the committed kernel constant reports no manifest
core digest, so every production call of `unc.create_model@1` and `unc.validate_model@1`
refuses with `kernel_manifest_unset`. The toy file is read only by test code, which pairs it
with a test kernel table that reports the toy core digest. It lists the four algorithms the
shared model fixtures use (the same IDs as the CHECK stub), with digests of labels that start
with "toy", so no field of it can be mistaken for a real build.

Rules the expected values follow (hand-labelled from the spec and the fid-unc docs):
  - canonical JSON: members sorted by key, no whitespace (all text is printable ASCII, so
    Python's code-point order equals RFC 8785's UTF-16 order); integers without a fraction;
  - a model of at most 8 MiB is one CORE.STO.CAS blob addressed by the SHA-256 of its bytes,
    so the event's `blob` equals the model digest and `blob_size` is the canonical length;
  - the `unc.create_model@1` journal event:
      {"author_kind","blob","blob_size","branch","by","command","digest",
       "kernel_manifest_digest","model"}
    with the author kind and identity of the bus-derived principal (here the human `alice`
    on branch `main`) and the digest of the release manifest the model was checked against;
  - results: create {"author_kind","digest","model","outcome"} with outcome `stored`,
    `already_stored` or `dry_run`; validate {"digest","findings","kernel_manifest_digest",
    "model","valid"}; show {"author_kind","canonical","digest","kernel_manifest_digest",
    "model"} where `canonical` is the stored canonical model text.

Files (in this directory):
  toy_release_manifest.json   the toy manifest, canonical bytes, no trailing newline
  expected.json               expected digests, events, results and findings

Usage: make_store_fixtures.py [--check]   (--check compares with the files on disk)
"""

import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
GRAPH = HERE.parent

TOY_NAME = "toy-test-only-not-a-release"
# The algorithm IDs the shared fixtures use (bore_ls.json and bad/*.json).
ALGORITHM_IDS = ["unc.op.scale@1.1.0", "fit.circle.ls@1.0.0", "unc.op.scale@1.0.0", "fit.cylinder.ls@1.0.0"]
CHUNK_SIZE = 8 * 1024 * 1024


def sha_label(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def sha_bytes(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canon(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return text.encode("ascii")


def alg_key(entry):
    name, version = entry["id"].split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


def toy_manifest():
    core = {
        "schema_version": "1",
        "name": TOY_NAME,
        "semver": "0.0.0",
        "numeric_profile": "knp-1",
        "source_tree_digest": sha_label("toy source tree"),
        "lockfile_digest": sha_label("toy Cargo.lock"),
        "toolchain_digest": sha_label("toy toolchain"),
        "replay_runtime": {"version": "0.0.0", "config_digest": sha_label("toy replay config")},
        "algorithms": sorted(
            ({"id": i, "card_digest": sha_label("toy card " + i), "impl_digest": sha_label("toy impl " + i)}
             for i in ALGORITHM_IDS),
            key=alg_key),
        "facade_selftest_digest": sha_label("toy facade self-test"),
        "solver_limits_digest": sha_label("toy solver limits"),
        "reference_suite_digest": sha_label("toy reference suite"),
        "reference_results_digest": sha_label("toy reference results"),
        "native_gate": "measured-only",
    }
    artifacts = [{"role": "wasm", "target": "wasm32-unknown-unknown", "sha256": sha_label("toy wasm")}]
    core_digest = sha_bytes(canon(core))
    manifest = {"artifacts": artifacts, "core": core, "core_digest": core_digest}
    return canon(manifest), core_digest


def read_digest(name):
    text = (GRAPH / name).read_text(encoding="ascii").strip()
    assert text.startswith("sha256:") and len(text) == 71, name
    return text


def findings_of(file):
    """The hand-labelled CHECK findings of bad/<file>, in order."""
    rows = (GRAPH / "bad" / "expected.txt").read_text(encoding="ascii").splitlines()
    return [r.split("\t", 1)[1] for r in rows if r.split("\t", 1)[0] == file]


def outputs():
    manifest_bytes, core_digest = toy_manifest()
    manifest_digest = sha_bytes(manifest_bytes)

    canonical = (GRAPH / "bore_ls.canon.json").read_bytes()
    digest = read_digest("bore_ls.sha256")
    assert sha_bytes(canonical) == digest, "bore_ls.canon.json does not match bore_ls.sha256"
    assert len(canonical) <= CHUNK_SIZE, "a single-blob model"
    param_digest = read_digest("bore_ls_param.sha256")
    model = "um:bore-ls@1.0.0"

    event = {
        "author_kind": "human",
        "blob": digest,
        "blob_size": len(canonical),
        "branch": "main",
        "by": "alice",
        "command": "unc.create_model@1",
        "digest": digest,
        "kernel_manifest_digest": manifest_digest,
        "model": model,
    }
    agent_event = dict(event, author_kind="agent", branch="agent/claude-client/1", by="claude-client")

    def create(outcome, kind="human"):
        return canon({"author_kind": kind, "digest": digest, "model": model, "outcome": outcome}).decode("ascii")

    cycle = findings_of("cycle.json")
    assert cycle == ["cycle n3 n3,n5,n7"], cycle

    expected = {
        "toy_core_digest": core_digest,
        "toy_manifest_digest": manifest_digest,
        "bore_ls_digest": digest,
        "bore_ls_param_digest": param_digest,
        "bore_ls_size": len(canonical),
        "ac1_event": canon(event).decode("ascii"),
        "ac1_result": create("stored"),
        "ac1_again_result": create("already_stored"),
        "dry_run_result": create("dry_run"),
        "agent_event": canon(agent_event).decode("ascii"),
        "agent_result": create("stored", "agent"),
        "ac3_findings": cycle,
        "ac4_show_result": canon({
            "author_kind": "human",
            "canonical": canonical.decode("ascii"),
            "digest": digest,
            "kernel_manifest_digest": manifest_digest,
            "model": model,
        }).decode("ascii"),
        "validate_bore_ls_result": canon({
            "digest": digest,
            "findings": [],
            "kernel_manifest_digest": manifest_digest,
            "model": model,
            "valid": True,
        }).decode("ascii"),
    }
    body = json.dumps(expected, sort_keys=True, indent=2, ensure_ascii=True) + "\n"
    return {"toy_release_manifest.json": manifest_bytes, "expected.json": body.encode("ascii")}


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    files = outputs()
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                print("stale: " + rel)
                bad += 1
        else:
            path.write_bytes(data)
    if bad:
        sys.exit(1)
    print("ok: %d files" % len(files))


if __name__ == "__main__":
    main()
