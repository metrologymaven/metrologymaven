#!/usr/bin/env python3
"""Independent reference for TRUST.REPLAY.SELECT (crate fid-vreplay, module select).

CC-BY-4.0 project data. Stdlib only. It never runs any Rust code.

Builds synthetic Result Packages (ZIP containers written by the TRUST.PKG.MANIFEST reference
`fixtures/pkg/make_fixtures.py`) and, for each, the expected selection: which results are
replayable, the ordered replay plan, and the one primary NOT_REPLAYABLE reason of every other
result. The expected selection is computed by `select()` below, a second statement of the
leaf's rules written from spec/trust/replay/select/NODE.md, and every case also carries hand
labels (`LABELS`) that the computed selection must match before anything is written.

Package content model assumed here (TRUST.PKG.CONTENT.SELECT and .JSEG are not built yet; see
the open questions in the leaf). The TRUST.PKG.FIXTURE golden packages follow it:

- `results/records.json`: a JSON array of ResultRecord objects; SELECT reads `id`,
  `evaluation_basis`, `algorithm` and `journal_entry` and ignores every other field;
- `journal/segment.jsonl`: one RFC 8785 line per entry, `{"header": <entry.schema.json header>,
  "payload": {"salts": {...}, "args": ..., ...}}`, the payload optional (detached);
- `points/<sha256 hex>.<ext>`: packaged point sets and other recorded outputs, by digest;
- `kernel/manifest.json`: the kernel manifest (schemas/kernel-manifest.schema.json), canonical.

Writes, under fixtures/replay/select/:
- `<case>.zip`                  the package;
- `<case>.plan.json`            canonical JSON of the expected selection (default options);
- `<case>.<variant>.plan.json`  the same with other options (artifact unavailable, small limits);
- `malformed.json`              packages the selector must refuse, with the expected error.

Run:  python fixtures/replay/select/make_cases.py           (write)
      python fixtures/replay/select/make_cases.py --check   (fail if any file is stale)
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
sys.path.insert(0, str(ROOT / "fixtures" / "pkg"))
import make_fixtures as mf  # noqa: E402  (ZIP container and package manifest writer)

ZERO = "sha256:" + "0" * 64
MAX_PLAN_CALLS = 100_000
WORK_BUDGET = 10_000_000

# Primary-reason precedence: the order REQ-TRUST.REPLAY.SELECT-001 lists them, then PLAN_TOO_LARGE.
ORDER = [
    "BASIS_RESULTS_ONLY",
    "BASIS_DIRECT_READING",
    "POINTS_MISSING",
    "ENTRY_MISSING",
    "ALGORITHM_UNKNOWN",
    "KERNEL_UNAVAILABLE",
    "NON_KERNEL_STEP",
    "PLAN_TOO_LARGE",
]


# --------------------------------------------------------------- canonical JSON (RFC 8785)
# Values here are strings, small non-negative integers, booleans, null, arrays and objects with
# ASCII keys, so RFC 8785 reduces to sorted keys, no whitespace, minimal string escaping and
# integers in plain decimal.

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


# --------------------------------------------------------------- kernel manifest

FIT = "fit.circle.ls@1.0.0"
DIST = "construct.distance.pair@1.0.0"
UNKNOWN = "fit.circle.minzone@9.0.0"


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
OTHER_KERNEL_DIGEST = sha(kernel_manifest([FIT, DIST], name="another-kernel").encode("utf-8"))


# --------------------------------------------------------------- journal

class Journal:
    """A linear journal segment. Entry index = 0-based line number."""

    def __init__(self):
        self.lines = []
        self.entries = []  # parsed model used by select()
        self.prev = ZERO
        self.add_session()

    def _add(self, kind, body_key, body, payload, kernel=KERNEL_DIGEST, tamper=None):
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
            "manifests": {"registry": label_digest("registry"), "kernel": kernel},
            body_key: body,
        }
        header["hash"] = sha(cbytes(header))
        if tamper:
            tamper(header)
        line = {"header": header}
        if payload is not None:
            line["payload"] = payload
        self.lines.append(canon(line))
        self.prev = header["hash"]
        self.entries.append({"header": header, "payload": payload})
        return header["hash"]

    def add_session(self):
        return self._add("session_header", "session", {"session_id": "s-1", "app_version": "0.0.0"}, None)

    def command(self, cid, algorithms, inputs, outputs, args, determinism="D0", cls="R1", seed=0,
                kernel=KERNEL_DIGEST, payload_mode="ok", tamper=None):
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
        if payload_mode == "detached":
            payload = None
        elif payload_mode == "no_args":
            payload = {"salts": {"events": s_events}, "events": events}
        elif payload_mode == "bad_args":
            payload = dict(payload, args={"method": "tampered"})
        return self._add("command", "command", body, payload, kernel=kernel, tamper=tamper)

    def text(self):
        return "".join(line + "\n" for line in self.lines)


def points_body(name):
    return f"synthetic point set {name}\n0x0000000000000000 0x3ff0000000000000 0x0000000000000000\n"


def digest_of(text):
    return sha(text.encode("utf-8"))


def record(rid, basis, algorithm, entry, inputs=()):
    return {
        "id": rid,
        "characteristic": "char-" + rid,
        "measurand": "diameter",
        "evaluation_basis": basis,
        "value": {"value": "0x4024000000000000", "unit": "mm"},
        "algorithm": algorithm,
        "kernel_version": "0.1.0",
        "input_digests": list(inputs),
        "journal_entry": entry,
    }


FIT_ARGS = {"method": "ls"}
DIST_ARGS = {"mode": "centre_to_centre"}


# --------------------------------------------------------------- the reference rules

def select(case, available=True, max_calls=MAX_PLAN_CALLS, work_budget=WORK_BUDGET):
    tier = case["tier"]
    entries = case["journal"].entries if case["journal"] else []
    points = set(case["points"])
    algs = case["algorithms"]
    kdigest = case["kernel_digest"]
    by_hash = {e["header"]["hash"]: i for i, e in enumerate(entries)}
    producers = {}
    for i, e in enumerate(entries):
        if e["header"]["kind"] == "command":
            for o in e["header"]["command"]["outputs"]:
                producers.setdefault(o["digest"], []).append(i)

    def is_kernel(i):
        h = entries[i]["header"]
        return h["kind"] == "command" and len(h["command"]["algorithms"]) == 1

    def producer(d, e):
        """(latest earlier kernel producer, latest earlier non-kernel producer); a kernel
        producer always wins, so a non-kernel re-emit can never hide a kernel step."""
        earlier = [i for i in producers.get(d, []) if i < e]
        kernel = [i for i in earlier if is_kernel(i)]
        other = [i for i in earlier if not is_kernel(i)]
        return (kernel[-1] if kernel else None), (other[-1] if other else None)

    budget = [work_budget]
    exhausted = [False]

    def spend():
        budget[0] -= 1
        if budget[0] < 0:
            exhausted[0] = True
        return not exhausted[0]

    results = []
    closures = {}
    for rec in case["records"]:
        rid = rec["id"]
        basis = rec["evaluation_basis"]

        def nr(reason, detail):
            results.append({"record_id": rid, "replayable": False, "reason": reason, "detail": detail})

        if basis == "RESULTS_ONLY":
            nr("BASIS_RESULTS_ONLY", "evaluation_basis=RESULTS_ONLY")
            continue
        if basis == "DIRECT_READING":
            nr("BASIS_DIRECT_READING", "evaluation_basis=DIRECT_READING")
            continue
        if tier == "results":
            nr("BASIS_RESULTS_ONLY", "evidence_tier=results")
            continue
        if exhausted[0]:
            nr("PLAN_TOO_LARGE", "work_budget")
            continue
        root = by_hash.get(rec["journal_entry"])
        if root is None:
            nr("ENTRY_MISSING", "not_found:" + rec["journal_entry"])
            continue
        rh = entries[root]["header"]
        if rh["kind"] != "command":
            nr("ENTRY_MISSING", "not_a_command:" + rh["hash"])
            continue
        if not is_kernel(root):
            nr("NON_KERNEL_STEP", "root_not_kernel:" + rh["hash"])
            continue
        if rh["command"]["algorithms"][0] != rec["algorithm"]:
            nr("ENTRY_MISSING", "algorithm_mismatch:" + rh["hash"])
            continue
        fails = {}
        too_large = None
        visited = set()
        substituted = set()
        stack = [root]
        while stack:
            e = stack.pop()
            if e in visited:
                continue
            visited.add(e)
            if not spend():
                too_large = "work_budget"
                break
            if len(visited) > max_calls:
                too_large = f"closure_calls>{max_calls}"
                break
            h = entries[e]["header"]
            c = h["command"]
            alg = c["algorithms"][0]
            if algs is None or alg not in algs:
                fails.setdefault("ALGORITHM_UNKNOWN", set()).add(alg)
            if h["manifests"]["kernel"] != kdigest:
                fails.setdefault("KERNEL_UNAVAILABLE", set()).add("kernel_manifest_mismatch:" + h["hash"])
            p = entries[e]["payload"]
            if p is None or "args" not in p or "args" not in p.get("salts", {}):
                fails.setdefault("ENTRY_MISSING", set()).add("args_missing:" + h["hash"])
            elif commitment(p["salts"]["args"], p["args"]) != c["args_commit"]:
                fails.setdefault("ENTRY_MISSING", set()).add("args_commitment:" + h["hash"])
            for inp in c["inputs"]:
                d = inp["digest"]
                if not spend():
                    too_large = "work_budget"
                    break
                kprod, prod = producer(d, e)
                if kprod is not None:
                    stack.append(kprod)
                elif d in points:
                    # Recorded output stands in (REQ-003); disclosed when a step produced it.
                    if prod is not None:
                        substituted.add((d, entries[prod]["header"]["hash"]))
                elif prod is None or entries[prod]["header"]["command"]["determinism"] == "N":
                    fails.setdefault("POINTS_MISSING", set()).add(d)
                else:
                    fails.setdefault("NON_KERNEL_STEP", set()).add(
                        "not_recomputable:" + entries[prod]["header"]["hash"])
            if too_large:
                break
        if too_large:
            nr("PLAN_TOO_LARGE", too_large)
            continue
        if not available:
            fails.setdefault("KERNEL_UNAVAILABLE", set()).add("artifact_unavailable:" + kdigest)
        primary = next((r for r in ORDER if r in fails), None)
        if primary:
            nr(primary, min(fails[primary]))
            continue
        closures[len(results)] = visited
        results.append({"record_id": rid, "replayable": True, "algorithm_id": rec["algorithm"], "calls": None,
                        "substituted": [{"digest": d, "producer": h} for d, h in sorted(substituted)]})

    used = sorted(set().union(*closures.values())) if closures else []
    pos = {e: k for k, e in enumerate(used)}
    calls = []
    for e in used:
        h = entries[e]["header"]
        c = h["command"]
        sources = []
        for i in c["inputs"]:
            kprod, _ = producer(i["digest"], e)
            if kprod is not None:
                outs = [o["digest"] for o in entries[kprod]["header"]["command"]["outputs"]]
                sources.append({"call": str(pos[kprod]), "output": str(outs.index(i["digest"]))})
            else:
                assert i["digest"] in points
                sources.append({"packaged": i["digest"]})
        calls.append({
            "entry_index": str(e),
            "entry_hash": h["hash"],
            "algorithm_id": c["algorithms"][0],
            "params": entries[e]["payload"]["args"],
            "seed": c["seed"],
            "inputs": [i["digest"] for i in c["inputs"]],
            "outputs": [o["digest"] for o in c["outputs"]],
            "sources": sources,
        })
    for k, visited in closures.items():
        results[k]["calls"] = [str(pos[e]) for e in sorted(visited)]
    return {"kernel_manifest_digest": kdigest, "calls": calls, "results": results}


# --------------------------------------------------------------- cases

def base_case(tier="points"):
    return {"tier": tier, "journal": Journal(), "points": {}, "records": [], "algorithms": {FIT, DIST},
            "kernel_digest": KERNEL_DIGEST, "kernel_body": KERNEL}


def add_points(case, name, packaged=True, determinism="N"):
    body = points_body(name)
    d = digest_of(body)
    case["journal"].command("io.points.import", [], [], [d], {"source": name}, determinism=determinism, cls="I1")
    if packaged:
        case["points"][d] = body
    return d


def fit(case, inp, name, algorithm=FIT, **kw):
    out = label_digest("circle " + name)
    h = case["journal"].command("gdt.fit_circle", [algorithm], [inp], [out], FIT_ARGS, **kw)
    return h, out


def case_points():
    c = base_case()
    ps = [add_points(c, f"hole-{i}") for i in (1, 2, 3)]
    for i, p in enumerate(ps, 1):
        h, _ = fit(c, p, f"hole-{i}")
        c["records"].append(record(f"hole-{i}", "POINTS", FIT, h, [p]))
    return c


def case_results():
    c = base_case(tier="results")
    c["points"] = {}
    c["journal"] = None
    c["records"] = [
        record("r-1", "RESULTS_ONLY", FIT, label_digest("absent entry 1")),
        record("r-2", "RESULTS_ONLY", DIST, label_digest("absent entry 2")),
        # A record claiming POINTS in a results-tier package is still BASIS_RESULTS_ONLY.
        record("r-3", "POINTS", FIT, label_digest("absent entry 3")),
    ]
    return c


def case_missing_points():
    c = base_case()
    ps = [add_points(c, f"hole-{i}", packaged=(i != 2)) for i in (1, 2, 3)]
    for i, p in enumerate(ps, 1):
        h, _ = fit(c, p, f"hole-{i}")
        c["records"].append(record(f"hole-{i}", "POINTS", FIT, h, [p]))
    return c


def case_constructed():
    c = base_case()
    pa = add_points(c, "circle-a")
    pb = add_points(c, "circle-b")
    ha, fa = fit(c, pa, "a")
    c["journal"].command("project.note", [], [], [label_digest("note")], {"text": "unrelated"})
    hb, fb = fit(c, pb, "b")
    hd = c["journal"].command("gdt.distance", [DIST], [fa, fb], [label_digest("distance a-b")], DIST_ARGS)
    c["records"] = [
        record("distance-ab", "POINTS", DIST, hd, [pa, pb]),
        record("circle-a", "POINTS", FIT, ha, [pa]),
        record("circle-b", "POINTS", FIT, hb, [pb]),
    ]
    return c


def case_unknown_algo():
    c = base_case()
    p1 = add_points(c, "hole-1")
    p2 = add_points(c, "hole-2")
    h1, _ = fit(c, p1, "hole-1", algorithm=UNKNOWN)
    h2, _ = fit(c, p2, "hole-2")
    c["records"] = [record("hole-1", "POINTS", UNKNOWN, h1, [p1]), record("hole-2", "POINTS", FIT, h2, [p2])]
    return c


def case_reasons():
    """Every other reason and detail, and the substitution and precedence rules."""
    c = base_case()
    j = c["journal"]
    p = add_points(c, "shared")
    recs = []
    # Basis reasons come first, whatever the entry.
    hfit, ffit = fit(c, p, "plain")
    recs.append(record("basis-dr", "DIRECT_READING", FIT, hfit))
    recs.append(record("basis-ro", "RESULTS_ONLY", FIT, hfit))
    recs.append(record("features-ok", "FEATURES", FIT, hfit, [p]))
    # The record's entry.
    recs.append(record("entry-not-found", "POINTS", FIT, label_digest("no such entry")))
    recs.append(record("entry-not-command", "POINTS", FIT, j.entries[0]["header"]["hash"]))
    recs.append(record("root-not-kernel", "POINTS", FIT, j.entries[1]["header"]["hash"]))
    hmulti = j.command("gdt.fit_circle", [DIST, FIT], [p], [label_digest("multi")], FIT_ARGS)
    recs.append(record("root-multi-algorithm", "POINTS", FIT, hmulti))
    recs.append(record("algorithm-mismatch", "POINTS", DIST, hfit))
    # Kernel-call checks inside the closure.
    hkm, _ = fit(c, p, "other kernel", kernel=OTHER_KERNEL_DIGEST)
    recs.append(record("kernel-manifest-mismatch", "POINTS", FIT, hkm))
    hdet, _ = fit(c, p, "detached", payload_mode="detached")
    recs.append(record("args-detached", "POINTS", FIT, hdet))
    hna, _ = fit(c, p, "no args", payload_mode="no_args")
    recs.append(record("args-missing", "POINTS", FIT, hna))
    hba, _ = fit(c, p, "bad args", payload_mode="bad_args")
    recs.append(record("args-commitment", "POINTS", FIT, hba))
    # Non-kernel producers: substituted when their output is packaged, otherwise a reason that
    # depends on the recorded class only for the label.
    sub_body = points_body("filtered, packaged")
    sub_out = digest_of(sub_body)
    c["points"][sub_out] = sub_body
    j.command("points.filter", [], [p], [sub_out], {"filter": "outliers"}, determinism="D0")
    hsub, _ = fit(c, sub_out, "from filtered")
    recs.append(record("substituted-non-kernel", "POINTS", FIT, hsub))
    gone = label_digest("filtered points, not packaged")
    hfilter = j.command("points.filter", [], [p], [gone], {"filter": "outliers 2"}, determinism="D0")
    hnr, _ = fit(c, gone, "from unpackaged filter")
    recs.append(record("not-recomputable", "POINTS", FIT, hnr))
    assert hfilter
    gone_n = label_digest("reading, not packaged")
    j.command("probe.read", [], [], [gone_n], {"probe": "p1"}, determinism="N", cls="D1")
    hpm, _ = fit(c, gone_n, "from unpackaged reading")
    recs.append(record("class-n-missing", "POINTS", FIT, hpm))
    hnp, _ = fit(c, label_digest("never produced"), "from nothing")
    recs.append(record("no-producer", "POINTS", FIT, hnp))
    # Precedence: POINTS_MISSING (listed first) beats ALGORITHM_UNKNOWN.
    hprec, _ = fit(c, label_digest("never produced 2"), "unknown and missing", algorithm=UNKNOWN)
    recs.append(record("precedence", "POINTS", UNKNOWN, hprec))
    # A kernel-produced input is recomputed even when its bytes are packaged too.
    fa_body = points_body("fit output, packaged")
    fa = digest_of(fa_body)
    c["points"][fa] = fa_body
    hka = j.command("gdt.fit_circle", [FIT], [p], [fa], FIT_ARGS)
    hkd = j.command("gdt.distance", [DIST], [fa, ffit], [label_digest("distance k")], DIST_ARGS, seed=7)
    recs.append(record("kernel-output-recomputed", "POINTS", DIST, hkd))
    assert hka
    # A failing dependency makes the dependent result not replayable too.
    hdep = j.command("gdt.distance", [DIST], [ffit, label_digest("circle other kernel")], [label_digest("d2")],
                     DIST_ARGS)
    recs.append(record("dependency-fails", "POINTS", DIST, hdep))
    hunk, _ = fit(c, p, "unknown only", algorithm=UNKNOWN)
    recs.append(record("algorithm-unknown", "POINTS", UNKNOWN, hunk))
    c["records"] = recs
    return c


def case_shadow():
    """Review probe: a kernel fit at 2, a non-kernel step re-emitting its output C at 4 (C
    packaged), the distance at 5. The fit is planned and C comes from it, not the package."""
    c = base_case()
    p = add_points(c, "probe")
    body = points_body("re-emitted circle")
    fc = digest_of(body)
    c["points"][fc] = body
    hfit = c["journal"].command("gdt.fit_circle", [FIT], [p], [fc], FIT_ARGS)
    q = add_points(c, "probe second")
    c["journal"].command("feature.export", [], [fc], [fc], {"format": "copy"}, determinism="D0")
    hd = c["journal"].command("gdt.distance", [DIST], [fc, q], [label_digest("distance probe")], DIST_ARGS)
    c["records"] = [record("distance", "POINTS", DIST, hd, [p, q]), record("circle", "POINTS", FIT, hfit, [p])]
    return c


CASES = {
    "points": case_points,
    "results": case_results,
    "missing-points": case_missing_points,
    "constructed": case_constructed,
    "unknown-algo": case_unknown_algo,
    "reasons": case_reasons,
    "shadow": case_shadow,
}

# Variants: (case, variant name, select() keyword arguments).
VARIANTS = [
    ("points", "unavailable", {"available": False}),
    ("points", "budget0", {"work_budget": 0}),
    ("constructed", "limit2", {"max_calls": 2}),
    ("constructed", "limit3", {"max_calls": 3}),
]

# Hand labels: record ID -> expected reason, or "ok" with the expected entry indices of its calls.
LABELS = {
    "points": {"hole-1": ("ok", [4]), "hole-2": ("ok", [5]), "hole-3": ("ok", [6])},
    "results": {"r-1": "BASIS_RESULTS_ONLY", "r-2": "BASIS_RESULTS_ONLY", "r-3": "BASIS_RESULTS_ONLY"},
    "missing-points": {"hole-1": ("ok", [4]), "hole-2": "POINTS_MISSING", "hole-3": ("ok", [6])},
    "constructed": {"distance-ab": ("ok", [3, 5, 6]), "circle-a": ("ok", [3]), "circle-b": ("ok", [5])},
    "unknown-algo": {"hole-1": "ALGORITHM_UNKNOWN", "hole-2": ("ok", [4])},
    "reasons": {
        "basis-dr": "BASIS_DIRECT_READING",
        "basis-ro": "BASIS_RESULTS_ONLY",
        "features-ok": ("ok", [2]),
        "entry-not-found": "ENTRY_MISSING",
        "entry-not-command": "ENTRY_MISSING",
        "root-not-kernel": "NON_KERNEL_STEP",
        "root-multi-algorithm": "NON_KERNEL_STEP",
        "algorithm-mismatch": "ENTRY_MISSING",
        "kernel-manifest-mismatch": "KERNEL_UNAVAILABLE",
        "args-detached": "ENTRY_MISSING",
        "args-missing": "ENTRY_MISSING",
        "args-commitment": "ENTRY_MISSING",
        "substituted-non-kernel": ("ok", [9]),
        "not-recomputable": "NON_KERNEL_STEP",
        "class-n-missing": "POINTS_MISSING",
        "no-producer": "POINTS_MISSING",
        "precedence": "POINTS_MISSING",
        "kernel-output-recomputed": ("ok", [2, 16, 17]),
        "dependency-fails": "KERNEL_UNAVAILABLE",
        "algorithm-unknown": "ALGORITHM_UNKNOWN",
    },
    "shadow": {"distance": ("ok", [2, 5]), "circle": ("ok", [2])},
    "points.unavailable": {"hole-1": "KERNEL_UNAVAILABLE", "hole-2": "KERNEL_UNAVAILABLE",
                           "hole-3": "KERNEL_UNAVAILABLE"},
    "points.budget0": {"hole-1": "PLAN_TOO_LARGE", "hole-2": "PLAN_TOO_LARGE", "hole-3": "PLAN_TOO_LARGE"},
    "constructed.limit2": {"distance-ab": "PLAN_TOO_LARGE", "circle-a": ("ok", [3]), "circle-b": ("ok", [5])},
    "constructed.limit3": {"distance-ab": ("ok", [3, 5, 6]), "circle-a": ("ok", [3]), "circle-b": ("ok", [5])},
}


def check_labels(name, sel):
    want = LABELS[name]
    got = {r["record_id"]: r for r in sel["results"]}
    assert set(want) == set(got), (name, set(want) ^ set(got))
    for rid, label in want.items():
        r = got[rid]
        if isinstance(label, tuple):
            assert r["replayable"], (name, rid, r)
            entries = [int(sel["calls"][int(k)]["entry_index"]) for k in r["calls"]]
            assert entries == label[1], (name, rid, entries, label)
        else:
            assert not r["replayable"] and r["reason"] == label, (name, rid, r, label)


def package_bytes(case, edit=None):
    members = []
    if case["records"] is not None:
        members.append({"path": "results/records.json", "media_type": "application/json",
                        "body": canon(case["records"])})
    if case["journal"] is not None:
        members.append({"path": "journal/segment.jsonl", "media_type": "application/x-ndjson",
                        "body": case["journal"].text()})
    for d, body in sorted(case["points"].items()):
        members.append({"path": "points/" + d[len("sha256:"):] + ".pts", "media_type": "text/plain", "body": body})
    if case["kernel_body"] is not None:
        members.append({"path": "kernel/manifest.json", "media_type": "application/json",
                        "body": case["kernel_body"]})
    members.append({"path": "profile/profile.json", "media_type": "application/json", "body": mf.PROFILE_BODY})
    contents = {
        "created_by": "fid-vreplay select fixture generator 1",
        "evidence_tier": case["tier"],
        "profile_digest": mf.digest_of(mf.PROFILE_BODY),
        "kernel_manifest_digest": case["kernel_digest"],
        "result_count": len(case["records"] or []),
        "members": members,
    }
    if edit:
        edit(contents)
    _, _, z = mf.build_package(contents)
    return z


# --------------------------------------------------------------- malformed packages

def set_member(path, body):
    def edit(contents):
        for m in contents["members"]:
            if m["path"] == path:
                m["body"] = body
                return
        contents["members"].append({"path": path, "media_type": "application/json", "body": body})
    return edit


def malformed_cases():
    out = []

    def tampered_seq(h):
        h["seq"] = "99"

    c = case_points()
    c["journal"].command("gdt.fit_circle", [FIT], [label_digest("x")], [label_digest("y")], FIT_ARGS,
                         tamper=tampered_seq)
    out.append(("entry-hash-mismatch", package_bytes(c), "pkg.malformed", "entry_hash_mismatch"))

    c = case_points()
    text = c["journal"].text().replace('{"header":', '{ "header":', 1)
    out.append(("non-canonical-line", package_bytes(c, set_member("journal/segment.jsonl", text)),
                "pkg.malformed", "non_canonical"))

    c = case_points()
    text = c["journal"].text() + "\n" + c["journal"].lines[-1] + "\n"
    out.append(("empty-line", package_bytes(c, set_member("journal/segment.jsonl", text)),
                "pkg.malformed", "empty_line"))

    c = case_points()
    text = c["journal"].text() + c["journal"].lines[-1] + "\n"
    out.append(("duplicate-entry", package_bytes(c, set_member("journal/segment.jsonl", text)),
                "pkg.malformed", "duplicate_entry_hash"))

    c = case_points()
    c["records"][0]["evaluation_basis"] = "SOMETIMES"
    out.append(("unknown-basis", package_bytes(c), "pkg.malformed", "unknown_basis"))

    c = case_points()
    c["records"][0].pop("journal_entry")
    out.append(("record-field-missing", package_bytes(c), "pkg.malformed", "record_field"))

    c = case_points()
    body = canon(c["records"])
    body = body.replace('{"algorithm":', '{"id":"dup","algorithm":', 1)
    out.append(("records-duplicate-key", package_bytes(c, set_member("results/records.json", body)),
                "pkg.malformed", "json"))

    c = case_points()

    def wrong_points_name(contents):
        for m in contents["members"]:
            if m["path"].startswith("points/"):
                m["path"] = "points/" + "ab" * 32 + ".pts"
                return

    out.append(("points-name", package_bytes(c, wrong_points_name), "pkg.malformed", "points_name"))

    c = case_points()
    c["kernel_digest"] = OTHER_KERNEL_DIGEST
    out.append(("kernel-manifest-digest", package_bytes(c), "pkg.malformed", "kernel_manifest_digest"))

    c = case_points()
    doc = json.loads(KERNEL)
    doc["core_digest"] = label_digest("wrong core")
    c["kernel_body"] = canon(doc)
    c["kernel_digest"] = sha(c["kernel_body"].encode("utf-8"))
    out.append(("kernel-core-digest", package_bytes(c), "pkg.malformed", "kernel_manifest"))

    c = case_points()
    c["records"][1]["id"] = c["records"][0]["id"]
    out.append(("duplicate-record-id", package_bytes(c), "pkg.malformed", "duplicate_record_id"))

    c = case_points()
    c["records"] = None
    out.append(("records-absent", package_bytes(c), "pkg.malformed", "records_missing"))
    return out


# --------------------------------------------------------------- output

# --------------------------------------------------------------- golden packages (AC-1, AC-2)
# AC-1 and AC-2 name the TRUST.PKG.FIXTURE golden packages `fixtures/pkg/golden/points.zip` and
# `results.zip`. They are read here with the stdlib `zipfile`, never through fid-pkg or any Rust
# code, and `select()` runs on their contents. The hand labels must hold, and the selection must
# equal the plan AC-1 names (`points.plan.json`) and the results plan (`results.plan.json`), so
# the Rust tests check both golden packages against those two files.

GOLDEN = ROOT / "fixtures" / "pkg" / "golden"
GOLDEN_CASES = {"points": "points.plan.json", "results": "results.plan.json"}
GOLDEN_LABELS = {
    # Three hole diameters, each fitted from its own packaged point set.
    "points": {"hole-1": ("ok", [4]), "hole-2": ("ok", [5]), "hole-3": ("ok", [6])},
    # Two RESULTS_ONLY imports and one POINTS record in a results-tier package.
    "results": {"r-1": "BASIS_RESULTS_ONLY", "r-2": "BASIS_RESULTS_ONLY", "r-3": "BASIS_RESULTS_ONLY"},
}


class Parsed:
    """A journal read back from a package: just the parsed entries select() walks."""

    def __init__(self, entries):
        self.entries = entries


def golden_case(name):
    import zipfile

    with zipfile.ZipFile(GOLDEN / f"{name}.zip") as z:
        names = set(z.namelist())
        manifest = json.loads(z.read("manifest.json"))
        for m in manifest["members"]:
            assert m["path"] in names, (name, m["path"])
            assert sha(z.read(m["path"])) == "sha256:" + m["sha256"], (name, m["path"])
        paths = [m["path"] for m in manifest["members"]]
        records = json.loads(z.read("results/records.json"))
        entries = []
        if "journal/segment.jsonl" in paths:
            text = z.read("journal/segment.jsonl").decode("utf-8")
            assert text.endswith("\n"), name
            for line in text.split("\n")[:-1]:
                v = json.loads(line)
                assert canon(v) == line, (name, "non-canonical line")
                h = dict(v["header"])
                stated = h.pop("hash")
                assert sha(cbytes(h)) == stated, (name, "entry hash")
                entries.append({"header": v["header"], "payload": v.get("payload")})
        kbody = z.read("kernel/manifest.json")
        assert sha(kbody) == manifest["kernel_manifest_digest"], (name, "kernel manifest digest")
        algs = {a["id"] for a in json.loads(kbody)["core"]["algorithms"]}
    points = {}
    for path in paths:
        if path.startswith("points/"):
            hexd = path[len("points/"):].split(".")[0]
            points["sha256:" + hexd] = path
    return {"tier": manifest["evidence_tier"], "journal": Parsed(entries) if entries else None,
            "points": points, "records": records, "algorithms": algs,
            "kernel_digest": manifest["kernel_manifest_digest"]}


def check_golden(files):
    for name, plan in GOLDEN_CASES.items():
        sel = select(golden_case(name))
        want = GOLDEN_LABELS[name]
        got = {r["record_id"]: r for r in sel["results"]}
        assert set(want) == set(got), (name, set(want) ^ set(got))
        for rid, label in want.items():
            r = got[rid]
            if isinstance(label, tuple):
                assert r["replayable"], (name, rid, r)
                entries = [int(sel["calls"][int(k)]["entry_index"]) for k in r["calls"]]
                assert entries == label[1], (name, rid, entries, label)
            else:
                assert not r["replayable"] and r["reason"] == label, (name, rid, r, label)
        assert cbytes(sel) == files[plan], f"golden {name}.zip: selection differs from {plan}"


def write_all():
    files = {}
    built = {}
    for name, make in CASES.items():
        case = make()
        built[name] = case
        sel = select(case)
        check_labels(name, sel)
        files[f"{name}.zip"] = package_bytes(case)
        files[f"{name}.plan.json"] = cbytes(sel)
    for name, variant, kw in VARIANTS:
        sel = select(built[name], **kw)
        check_labels(f"{name}.{variant}", sel)
        files[f"{name}.{variant}.plan.json"] = cbytes(sel)
    listing = []
    for name, z, code, reason in malformed_cases():
        files[f"malformed/{name}.zip"] = z
        listing.append({"name": name, "code": code, "reason": reason})
    files["malformed.json"] = (json.dumps(listing, indent=2) + "\n").encode("utf-8")
    check_golden(files)
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    files = write_all()
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        known = {str(p.relative_to(HERE)).replace("\\", "/") for p in HERE.rglob("*")
                 if p.is_file() and p.name != "make_cases.py"}
        extra = sorted(known - set(files))
        if extra:
            print("unexpected files:", ", ".join(extra))
            return 1
    if stale:
        print("stale fixtures:", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
