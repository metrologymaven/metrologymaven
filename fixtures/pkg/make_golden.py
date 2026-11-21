#!/usr/bin/env python3
"""Independent reference for the TRUST.PKG.FIXTURE golden packages.

Writes, without running any Rust code:

- golden/<name>.zip         one synthetic Result Package per conformance role;
- golden/<name>.zip.sha256  its pinned SHA-256 (`sha256sum` format);
- golden/index.json         RFC 8785 canonical index of the goldens: file, SHA-256, evidence
                            tier, result count, signature count, the UEC-0 record IDs and
                            what each package is for.

Goldens (REQ-TRUST.PKG.FIXTURE-002):

- results.zip         evidence tier `results`: three records, two RESULTS_ONLY imports and one
                      POINTS result whose points the tier leaves out;
- features.zip        evidence tier `features`: a FEATURES distance between two fitted circles;
- points.zip          evidence tier `points`: three hole diameters, each fitted from its own
                      packaged point set;
- two-signatures.zip  points.zip plus two DSSE envelopes under `signatures/` (one P-256, one
                      Ed25519), each over a signature statement whose subject is statement.json;
- uec0.zip            evidence tier `points`: one UEC-2 result and one UEC-0 result, whose
                      uncertainty is `{"class":"UEC-0","status":"not_evaluated"}` with no
                      numeric field (REQ-TRUST.PKG-006).

Content model. TRUST.PKG.CONTENT.* is not built yet; this script follows what TRUST.REPLAY.SELECT
(PR #421, fixtures/replay/select/make_cases.py) assumed, so its expected plans hold for these
packages too (see the decision in spec/trust/pkg/fixture/NODE.md):

- `results/records.json`: an RFC 8785 JSON array of ResultRecords sorted by `id`, each with the
  CONSTITUTION section 5 minimum fields (a superset of the fields SELECT reads);
- `results/results.qif`: a clearly labelled placeholder until IO.RES.QIFR.ROUT defines it;
- `journal/segment.jsonl`: one RFC 8785 line per entry, `{"header": ..., "payload": ...}`, the
  header hash-chained (`prev`, `hash` = SHA-256 of the canonical header without `hash`), starting
  at the session header (no checkpoint precedes it, TRUST.PKG.CONTENT.JSEG-002);
- `points/<sha256 hex>.pts`: packaged point sets, named by the digest of their bytes;
- `kernel/manifest.json`: a synthetic kernel manifest (schemas/kernel-manifest.schema.json keys);
- `profile/profile.json`: the synthetic profile of fixtures/pkg/make_fixtures.py.

Containers come from the TRUST.PKG.MANIFEST reference writer fixtures/pkg/make_fixtures.py
(fixed 1980-01-01 timestamps, members sorted by path bytes, stored or stored-block deflate chosen
by media type only). Signatures come from fixtures/dsse/make_fixtures.py with the published RFC
8032 / RFC 6979 test keys in fixtures/dsse/keys: TEST KEYS ONLY, they prove nothing. No new key is
made here. Everything is synthetic (D-038). CC-BY-4.0 project data. Stdlib only.

Run:  python fixtures/pkg/make_golden.py          (writes the goldens)
      python fixtures/pkg/make_golden.py --check  (fails if any golden would change)
"""

import hashlib
import importlib.util
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
import make_fixtures as mf  # noqa: E402  (TRUST.PKG.MANIFEST reference writer)


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


dsse = _load("pkg_golden_dsse_ref", "fixtures/dsse/make_fixtures.py")

ZERO = "sha256:" + "0" * 64
CREATED_BY = "fid-pkg golden fixture generator 1 (TRUST.PKG.FIXTURE, synthetic test data)"
PAYLOAD_TYPE = "application/vnd.in-toto+json"
STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
SIG_PREDICATE_TYPE = "https://metrologymaven.io/signature/predicate/v1"


# --------------------------------------------------------------- canonical JSON (RFC 8785)
# Values: strings, non-negative integers below 2^53, booleans, null, arrays, objects with ASCII
# keys. RFC 8785 then reduces to sorted keys, no whitespace, minimal escaping, plain integers.

def canon(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        assert 0 <= v < 2 ** 53
        return str(v)
    if isinstance(v, str):
        return mf.jstr(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(mf.jstr(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(f"unexpected JSON value {v!r}")


def cbytes(v):
    return canon(v).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def label_digest(label):
    return sha(b"label:" + label.encode("utf-8"))


def salt(label):
    return hashlib.sha256(b"salt:" + label.encode("utf-8")).hexdigest()[:32]


def commitment(salt_hex, value):
    return sha(bytes.fromhex(salt_hex) + cbytes(value))


def f64(x):
    """binary64 bits as 0x-prefixed lowercase hex (the repository's lossless number form)."""
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


# --------------------------------------------------------------- kernel manifest

FIT = "fit.circle.ls@1.0.0"
DIST = "construct.distance.pair@1.0.0"


def kernel_manifest(algorithm_ids, name="synthetic-test-kernel"):
    core = {
        "algorithms": [
            {"card_digest": label_digest("card " + a), "id": a, "impl_digest": label_digest("impl " + a)}
            for a in sorted(algorithm_ids)
        ],
        "facade_selftest_digest": label_digest("facade"),
        "lockfile_digest": label_digest("lockfile"),
        "name": name,
        "native_gate": "measured-only",
        "numeric_profile": "knp1",
        "reference_results_digest": label_digest("reference results"),
        "reference_suite_digest": label_digest("reference suite"),
        "replay_runtime": {"config_digest": label_digest("runtime config"), "version": "1.0.0"},
        "schema_version": "1",
        "semver": "0.1.0",
        "solver_limits_digest": label_digest("solver limits"),
        "source_tree_digest": label_digest("source tree"),
        "toolchain_digest": label_digest("toolchain"),
    }
    doc = {
        "artifacts": [{"role": "reference", "sha256": label_digest("wasm " + name), "target": "wasm32-wasip1"}],
        "core": core,
        "core_digest": sha(cbytes(core)),
    }
    return canon(doc)


KERNEL = kernel_manifest([FIT, DIST])
KERNEL_DIGEST = sha(KERNEL.encode("utf-8"))
KERNEL_VERSION = "0.1.0"


# --------------------------------------------------------------- journal segment

class Journal:
    """A linear journal segment starting at its session header; entry index = line number."""

    def __init__(self):
        self.lines = []
        self.prev = ZERO
        self._add("session_header", "session", {"session_id": "s-1", "app_version": "0.0.0"}, None)

    def _add(self, kind, body_key, body, payload):
        seq = len(self.lines) + 1
        header = {
            "schema_version": 1,
            "kind": kind,
            "stream": "main",
            "seq": str(seq),
            "prev": self.prev,
            "branch": "main",
            "logical_parent": None,
            "time": {"utc": "2026-01-01T00:00:00Z", "mono_ns": str(seq * 1000), "sync": "test"},
            "principal": {"kind": "script", "commit": label_digest("principal")},
            "ai_assistance": "none",
            "manifests": {"registry": label_digest("registry"), "kernel": KERNEL_DIGEST},
            body_key: body,
        }
        header["hash"] = sha(cbytes(header))
        line = {"header": header}
        if payload is not None:
            line["payload"] = payload
        self.lines.append(canon(line))
        self.prev = header["hash"]
        return header["hash"]

    def command(self, cid, algorithms, inputs, outputs, args, determinism="D0", cls="R1", seed=0):
        n = len(self.lines)
        s_args, s_events = salt(f"args {n}"), salt(f"events {n}")
        events = []
        body = {
            "id": cid,
            "major": 1,
            "class": cls,
            "determinism": determinism,
            "seed": str(seed),
            "args_commit": commitment(s_args, args),
            "events_commit": commitment(s_events, events),
            "inputs": [{"tag": "input", "digest": d} for d in inputs],
            "outputs": [{"digest": d} for d in outputs],
            "algorithms": list(algorithms),
        }
        payload = {"salts": {"args": s_args, "events": s_events}, "args": args, "events": events}
        return self._add("command", "command", body, payload)

    def text(self):
        return "".join(line + "\n" for line in self.lines)


def points_body(name):
    return f"synthetic point set {name}\n0x0000000000000000 0x3ff0000000000000 0x0000000000000000\n"


FIT_ARGS = {"method": "ls"}
DIST_ARGS = {"mode": "centre_to_centre"}


class Case:
    def __init__(self, tier):
        self.tier = tier
        self.journal = Journal()
        self.points = {}
        self.records = []

    def import_points(self, name, packaged=True):
        body = points_body(name)
        d = sha(body.encode("utf-8"))
        self.journal.command("io.points.import", [], [], [d], {"source": name}, determinism="N", cls="I1")
        if packaged:
            self.points[d] = body
        return d

    def fit(self, inp, name):
        out = label_digest("circle " + name)
        return self.journal.command("gdt.fit_circle", [FIT], [inp], [out], FIT_ARGS), out


# --------------------------------------------------------------- ResultRecords (CONSTITUTION 5)

SPEC = {"defaults_profile": "dp:asme-y14.5-2018@1.0.0", "standards": ["std:asme:y14.5:2018"]}
UEC0 = {"class": "UEC-0", "status": "not_evaluated"}


def uec2(u_mm):
    return {"class": "UEC-2", "expanded": {"U": f64(u_mm), "k": f64(2.0)}, "method": "LPU", "unit": "mm"}


def record(rid, basis, algorithm, entry, inputs, value_mm=10.0, uncertainty=None, decision=None,
           measurand="diameter"):
    return {
        "id": rid,
        "characteristic": "char-" + rid,
        "measurand": measurand,
        "evaluation_basis": basis,
        "value": {"value": f64(value_mm), "unit": "mm"},
        "uncertainty": uncertainty if uncertainty is not None else uec2(0.002),
        "decision": decision if decision is not None else {"outcome": "CONFORMS", "rule": "dr:simple-acceptance@1"},
        "spec": SPEC,
        "algorithm": algorithm,
        "kernel_version": KERNEL_VERSION,
        "input_digests": list(inputs),
        "journal_entry": entry,
    }


QIF_PLACEHOLDER = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    "<!-- Synthetic placeholder written by fixtures/pkg/make_golden.py. Not a QIF Results document:\n"
    "     IO.RES.QIFR.ROUT will define this member's content. -->\n"
    "<QIFDocument/>\n"
)


# --------------------------------------------------------------- the goldens

def case_points():
    """Three holes, each fitted from its own packaged point set. Journal, points and kernel are
    byte-identical to the TRUST.REPLAY.SELECT `points` case (PR #421)."""
    c = Case("points")
    ps = [c.import_points(f"hole-{i}") for i in (1, 2, 3)]
    for i, p in enumerate(ps, 1):
        h, _ = c.fit(p, f"hole-{i}")
        c.records.append(record(f"hole-{i}", "POINTS", FIT, h, [p], value_mm=10.0 + i / 1000))
    return c


def case_results():
    """Two RESULTS_ONLY imports and one POINTS result in a `results` tier package: no points."""
    c = Case("results")
    imported = [label_digest("imported result r-1"), label_digest("imported result r-2")]
    hi = c.journal.command("io.results.import", [], [], imported, {"source": "synthetic-report-1"},
                           determinism="N", cls="I1")
    p = c.import_points("hole-3", packaged=False)
    hf, _ = c.fit(p, "hole-3")
    as_reported = {"outcome": "CONFORMS", "rule": "dr:as-reported-by-source@1"}
    c.records = [
        record("r-1", "RESULTS_ONLY", FIT, hi, [imported[0]], value_mm=12.0, decision=as_reported),
        record("r-2", "RESULTS_ONLY", DIST, hi, [imported[1]], value_mm=40.0, decision=as_reported,
               measurand="distance"),
        # A POINTS result exported at tier `results`: its points stay out of the package.
        record("r-3", "POINTS", FIT, hf, [p], value_mm=10.003),
    ]
    return c


def case_features():
    """A distance between two fitted circles at tier `features`, plus one of the circles."""
    c = Case("features")
    pa = c.import_points("circle-a", packaged=False)
    pb = c.import_points("circle-b", packaged=False)
    ha, fa = c.fit(pa, "a")
    _, fb = c.fit(pb, "b")
    hd = c.journal.command("gdt.distance", [DIST], [fa, fb], [label_digest("distance a-b")], DIST_ARGS)
    c.records = [
        record("circle-a", "POINTS", FIT, ha, [pa], value_mm=8.0),
        record("distance-ab", "FEATURES", DIST, hd, [fa, fb], value_mm=40.0, measurand="distance"),
    ]
    return c


def case_uec0():
    """Two holes: hole-1 with a UEC-2 statement, hole-2 not evaluated (UEC-0)."""
    c = Case("points")
    p1 = c.import_points("uec-hole-1")
    p2 = c.import_points("uec-hole-2")
    h1, _ = c.fit(p1, "uec-hole-1")
    h2, _ = c.fit(p2, "uec-hole-2")
    c.records = [
        record("hole-1", "POINTS", FIT, h1, [p1], value_mm=6.001),
        # UEC-0: "not evaluated", no numeric uncertainty, and a guard-band decision that needs
        # an uncertainty therefore cannot decide.
        record("hole-2", "POINTS", FIT, h2, [p2], value_mm=6.002, uncertainty=dict(UEC0),
               decision={"outcome": "NOT_DECIDABLE", "rule": "dr:guard-band@1"}),
    ]
    return c


def contents_of(case):
    records = sorted(case.records, key=lambda r: r["id"].encode("utf-8"))
    members = [
        {"path": "results/records.json", "media_type": "application/json", "body": canon(records)},
        {"path": "results/results.qif", "media_type": "application/xml", "body": QIF_PLACEHOLDER},
        {"path": "journal/segment.jsonl", "media_type": "application/x-ndjson", "body": case.journal.text()},
        {"path": "kernel/manifest.json", "media_type": "application/json", "body": KERNEL},
        {"path": "profile/profile.json", "media_type": "application/json", "body": mf.PROFILE_BODY},
    ]
    for d, body in sorted(case.points.items()):
        members.append({"path": "points/" + d[len("sha256:"):] + ".pts", "media_type": "text/plain", "body": body})
    return {
        "created_by": CREATED_BY,
        "evidence_tier": case.tier,
        "profile_digest": mf.digest_of(mf.PROFILE_BODY),
        "kernel_manifest_digest": KERNEL_DIGEST,
        "result_count": len(records),
        "members": members,
    }


# --------------------------------------------------------------- signatures (test keys only)

ED_SEED = bytes.fromhex(dsse.RFC8032[0]["secret"])
ED_FP = dsse.fingerprint(dsse.spki_ed25519(dsse.ed_public(ED_SEED)))
P_X = dsse.RFC6979_X
P_FP = dsse.fingerprint(dsse.spki_p256(dsse.p_public(P_X)))
# Signer-asserted signing times inside the validity windows of fixtures/troot/roots/both.json.
SIGNERS = [
    ("0.dsse.json", "p256", "2026-03-01T10:00:00Z", "reviewed"),
    ("1.dsse.json", "ed25519", "2026-03-02T09:30:00Z", "approved"),
]


def sign(which, message):
    if which == "p256":
        r, s = dsse.p_sign(P_X, message)
        return P_FP, dsse.ecdsa_der(r, dsse.low_s(s))
    return ED_FP, dsse.ed_sign(ED_SEED, message)


def signature_envelopes(statement_bytes):
    """One envelope per signer over a signature statement whose single subject is statement.json
    (the multi-signer form TRUST.SIGN.TROOT checks)."""
    out = []
    for name, which, signed_at, meaning in SIGNERS:
        fp = P_FP if which == "p256" else ED_FP
        payload = cbytes({
            "_type": STATEMENT_TYPE,
            "subject": [{"name": "statement.json", "digest": {"sha256": hashlib.sha256(statement_bytes).hexdigest()}}],
            "predicateType": SIG_PREDICATE_TYPE,
            "predicate": {"key_fingerprint": fp, "meaning_code": meaning, "signed_at": signed_at},
        })
        keyid, sig = sign(which, dsse.pae(PAYLOAD_TYPE.encode("ascii"), payload))
        out.append(("signatures/" + name, dsse.envelope(payload, PAYLOAD_TYPE, [(keyid, sig)])))
    return out


def signed_zip(contents, envelopes, statement_bytes, manifest_bytes):
    """The package of `contents` with `signatures/*` entries added after statement.json."""
    members = sorted(((m["path"], m["media_type"], mf.body_of(m)) for m in contents["members"]),
                     key=lambda t: t[0].encode("utf-8"))
    entries = [("manifest.json", "application/json", manifest_bytes)] + members
    entries.append(("statement.json", "application/json", statement_bytes))
    entries += [(name, "application/json", data) for name, data in envelopes]
    return mf.build_zip(entries)


# --------------------------------------------------------------- output

GOLDENS = [
    ("features", case_features, False, "Evidence tier features: a FEATURES distance between two fitted circles "
                                       "and one POINTS circle whose points the tier leaves out."),
    ("points", case_points, False, "Evidence tier points: three hole diameters, each fitted from its own "
                                   "packaged point set (TRUST.REPLAY.SELECT AC-1)."),
    ("results", case_results, False, "Evidence tier results: two RESULTS_ONLY imports and one POINTS result "
                                     "without its points (TRUST.REPLAY.SELECT AC-2)."),
    ("two-signatures", case_points, True, "points.zip with two DSSE envelopes under signatures/ (P-256 and "
                                          "Ed25519 published RFC test keys; TEST KEYS ONLY)."),
    ("uec0", case_uec0, False, "Evidence tier points: one UEC-2 result and one UEC-0 result recorded as "
                               "not evaluated, with no numeric uncertainty."),
]


def write_all():
    files = {}
    index = []
    for name, make, signed, purpose in GOLDENS:
        case = make()
        contents = contents_of(case)
        mbytes, sbytes, zbytes = mf.build_package(contents)
        envelopes = []
        if signed:
            envelopes = signature_envelopes(sbytes)
            zbytes = signed_zip(contents, envelopes, sbytes, mbytes)
        files[f"golden/{name}.zip"] = zbytes
        files[f"golden/{name}.zip.sha256"] = f"{hashlib.sha256(zbytes).hexdigest()}  {name}.zip\n".encode("ascii")
        index.append({
            "name": name,
            "file": f"{name}.zip",
            "sha256": hashlib.sha256(zbytes).hexdigest(),
            "evidence_tier": case.tier,
            "result_count": len(case.records),
            "record_ids": sorted((r["id"] for r in case.records), key=lambda s: s.encode("utf-8")),
            "uec0_record_ids": sorted(r["id"] for r in case.records if r["uncertainty"]["class"] == "UEC-0"),
            "signature_count": len(envelopes),
            "signatures": [name for name, _ in envelopes],
            "statement_sha256": hashlib.sha256(sbytes).hexdigest(),
            "purpose": purpose,
        })
    files["golden/index.json"] = cbytes(index)
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(write_all().items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures:", ", ".join(stale))
        return 1
    print(f"golden packages {'up to date' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
