#!/usr/bin/env python3
"""Independent reference for UNC.MODEL.GRAPH.CHECK fixtures (stdlib only; CC-BY-4.0 data).

A second, separate implementation of the structural validation passes pinned in
spec/unc/model/graph/check/NODE.md and in the module documentation of fid-kunc
`model::check`, written from those rules and never from the Rust code. Every invalid case
below also carries the findings a person wrote down by hand from the same rules; the script
fails unless this oracle reproduces each hand label exactly, and only then writes the files.

Passes (sorted by pass, then node ID, then the remaining fields; all text is ASCII, so
Python's str order is byte order):
  1 dangling_edge <to.node> <to.port> <from.node> <from.port>
  2 cycle <lowest member> <members joined by ','>        (one per cyclic strong component)
  3 unknown_operator <node> <algorithm_id>@<algorithm_version>
  4 unsourced_input <node>                               (no source, or a zero or empty-input digest)
  5 arity_mismatch <node> <port> <arity> <connected>     (sorted before unit_mismatch on a port)
  5 unit_mismatch <node> <port> <from.node> <from.port>  (quantity kind or unit code differs)
  6 unreachable_output <node>
  7 correlation_not_input <group> <member>               (group = its smallest input ID)
  8 invalid_correlation <group> <reason>                 (UNC.MODEL.DIST order: shape,
                                                          non_finite, asymmetric, diagonal,
                                                          off_diagonal_range, indefinite)

The indefiniteness decision uses a plain Cholesky factorisation and refuses any matrix whose
smallest pivot is within 1e-6 of zero, so every labelled case is far from the DIST tolerance.

Files (in this directory):
  manifest_stub.json     canonical kernel manifest (KRN.MAN.MANI.SCHEMA rules) listing the
                         algorithms the fixtures use; core_digest from hashlib
  bore_ls.order.txt      Kahn order of bore_ls.json (smallest ready ID first), one ID per line
  bad/<case>.json        models in the uncertainty-model/v1 form that parse but fail validation
  bad/expected.txt       "<file>\t<finding>" lines, each file's findings in order

The schema's own fixtures (bore_ls.json and friends) come from make_fixtures.py, whose model
builder this script reuses for the base model (bore_ls.json, which must validate).

Usage: make_check_fixtures.py [--check]   (--check compares with the files on disk)
"""

import copy
import hashlib
import json
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import make_fixtures as schema  # noqa: E402  (the SCHEMA oracle: base model and shuffle)

BAD = HERE / "bad"


def sha(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


# ------------------------------------------------------------------ manifest stub

def canon(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert "\\" not in text
    return text.encode("ascii")


def alg_key(entry):
    name, version = entry["id"].split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


MANIFEST_IDS = ["unc.op.scale@1.1.0", "fit.circle.ls@1.0.0", "unc.op.scale@1.0.0", "fit.cylinder.ls@1.0.0"]


def manifest_stub():
    core = {
        "schema_version": "1",
        "name": "fid-kernel-stub",
        "semver": "0.1.0",
        "numeric_profile": "knp-1",
        "source_tree_digest": sha("stub source tree"),
        "lockfile_digest": sha("stub Cargo.lock"),
        "toolchain_digest": sha("stub toolchain"),
        "replay_runtime": {"version": "0.1.0", "config_digest": sha("stub replay config")},
        "algorithms": sorted(
            ({"id": i, "card_digest": sha("card " + i), "impl_digest": sha("impl " + i)} for i in MANIFEST_IDS),
            key=alg_key),
        "facade_selftest_digest": sha("stub facade self-test"),
        "solver_limits_digest": sha("stub solver limits"),
        "reference_suite_digest": sha("stub reference suite"),
        "reference_results_digest": sha("stub reference results"),
        "native_gate": "measured-only",
    }
    artifacts = [{"role": "wasm", "target": "wasm32-unknown-unknown", "sha256": sha("stub wasm")}]
    return {
        "artifacts": artifacts,
        "core": core,
        "core_digest": "sha256:" + hashlib.sha256(canon(core)).hexdigest(),
    }


# ------------------------------------------------------------------ the oracle

def f64(hexbits):
    return struct.unpack(">d", bytes.fromhex(hexbits))[0]


def correlation_reason(matrix):
    """UNC.MODEL.DIST check order; None when valid. Clear-cut matrices only."""
    n = len(matrix)
    if n == 0 or any(len(r) != n for r in matrix):
        return "shape"
    m = [[f64(x) for x in r] for r in matrix]
    if any(x != x or x in (float("inf"), float("-inf")) for r in m for x in r):
        return "non_finite"
    if any(m[i][j] != m[j][i] for i in range(n) for j in range(i + 1, n)):
        return "asymmetric"
    if any(m[i][i] != 1.0 for i in range(n)):
        return "diagonal"
    if any(abs(x) > 1.0 for r in m for x in r):
        return "off_diagonal_range"
    # Plain Cholesky; the smallest pivot decides, and it must be far from zero either way.
    a = [row[:] for row in m]
    smallest = None
    for k in range(n):
        pivot = a[k][k] - sum(a[k][j] * a[k][j] for j in range(k))
        smallest = pivot if smallest is None else min(smallest, pivot)
        if pivot <= 0:
            break
        a[k][k] = pivot ** 0.5
        for i in range(k + 1, n):
            a[i][k] = (a[i][k] - sum(a[i][j] * a[k][j] for j in range(k))) / a[k][k]
    assert abs(smallest) > 1e-6, "borderline matrix: not a clear-cut fixture"
    return "indefinite" if smallest < 0 else None


def oracle(doc, manifest):
    """Findings of `doc` (a parsed model document) as (key, line) pairs, sorted."""
    inputs = {n["id"]: n for n in doc["inputs"]}
    ops = {n["id"]: n for n in doc["operators"]}
    outs = {n["id"]: n for n in doc["outputs"]}
    known = {a["id"] for a in manifest["core"]["algorithms"]}
    found = []

    def add(pass_no, node, rest, line, nums=(0, 0)):
        found.append(((pass_no, node, rest, nums), line))

    def source_port(node, port):
        if node in inputs and port == "value":
            n = inputs[node]
            return (n["quantity_kind"], n["unit"])
        if node in ops:
            for p in ops[node]["ports_out"]:
                if p["name"] == port:
                    return (p["quantity_kind"], p["unit"])
        return None

    def target_port(node, port):
        if node in outs and port == "value":
            n = outs[node]
            return (n["quantity_kind"], n["unit"], 1)
        if node in ops:
            for p in ops[node]["ports_in"]:
                if p["name"] == port:
                    return (p["quantity_kind"], p["unit"], int(p["arity"]))
        return None

    succ = {k: [] for k in list(inputs) + list(ops) + list(outs)}
    connected = {}
    for e in doc["edges"]:
        fn, fp, tn, tp = e["from"]["node"], e["from"]["port"], e["to"]["node"], e["to"]["port"]
        s, t = source_port(fn, fp), target_port(tn, tp)
        if t is not None:
            connected[(tn, tp)] = connected.get((tn, tp), 0) + 1
        if s is None or t is None:
            add(1, tn, [tp, fn, fp], "dangling_edge %s %s %s %s" % (tn, tp, fn, fp))
            continue
        succ[fn].append(tn)
        if s[0] != t[0] or s[1] != t[1]:
            add(5, tn, [tp, "1", fn, fp], "unit_mismatch %s %s %s %s" % (tn, tp, fn, fp))

    # Cycles: a node is on a cycle when it reaches itself; its component is everything it
    # reaches that also reaches it (quadratic, fine for fixtures, and unlike Tarjan).
    def reach(v):
        seen, todo = set(), list(succ[v])
        while todo:
            w = todo.pop()
            if w not in seen:
                seen.add(w)
                todo.extend(succ[w])
        return seen

    reaches = {v: reach(v) for v in succ}
    reported = set()
    for v in sorted(succ):
        if v in reaches[v] and v not in reported:
            comp = sorted(w for w in succ if w in reaches[v] and v in reaches[w])
            reported.update(comp)
            add(2, comp[0], comp, "cycle %s %s" % (comp[0], ",".join(comp)))

    for k, op in ops.items():
        alg = "%s@%s" % (op["algorithm_id"], op["algorithm_version"])
        if alg not in known:
            add(3, k, [alg], "unknown_operator %s %s" % (k, alg))

    empty = {"sha256:" + "0" * 64, "sha256:" + hashlib.sha256(b"").hexdigest()}
    for k, n in inputs.items():
        if "source" not in n or n["source"]["digest"] in empty:
            add(4, k, [], "unsourced_input %s" % k)

    ports = [(k, p["name"], int(p["arity"])) for k, op in ops.items() for p in op["ports_in"]]
    ports += [(k, "value", 1) for k in outs]
    for k, name, arity in ports:
        c = connected.get((k, name), 0)
        if c != arity:
            add(5, k, [name, "0"], "arity_mismatch %s %s %d %d" % (k, name, arity, c), (arity, c))

    from_inputs = set(inputs)
    for v in inputs:
        from_inputs |= reaches[v]
    for k in outs:
        if k not in from_inputs:
            add(6, k, [], "unreachable_output %s" % k)

    for g in doc["correlations"]:
        group = min(g["inputs"])
        for x in g["inputs"]:
            if x not in inputs:
                add(7, group, [x], "correlation_not_input %s %s" % (group, x))
        reason = correlation_reason(g["matrix"])
        if reason is not None:
            add(8, group, [reason], "invalid_correlation %s %s" % (group, reason))

    found.sort()
    return [line for _, line in found]


def kahn_order(doc):
    ids = sorted([n["id"] for n in doc["inputs"]] + [n["id"] for n in doc["operators"]] + [n["id"] for n in doc["outputs"]])
    indeg = {k: 0 for k in ids}
    succ = {k: [] for k in ids}
    for e in doc["edges"]:
        succ[e["from"]["node"]].append(e["to"]["node"])
        indeg[e["to"]["node"]] += 1
    ready, order = sorted(k for k in ids if indeg[k] == 0), []
    while ready:
        v = ready.pop(0)
        order.append(v)
        for w in succ[v]:
            indeg[w] -= 1
            if indeg[w] == 0:
                ready = sorted(ready + [w])
    assert len(order) == len(ids)
    return order


# ------------------------------------------------------------------ hand-labelled cases

edge = schema.edge
mat = schema.mat


def ring_op(ident):
    return {
        "id": ident,
        "algorithm_id": "unc.op.scale",
        "algorithm_version": "1.0.0",
        "params": {"mode": "multiplicative"},
        "ports_in": [schema.port_in("value", "length", "si:m", 1)],
        "ports_out": [schema.port_out("value", "length", "si:m")],
    }


def op(m, ident):
    return next(o for o in m["operators"] if o["id"] == ident)


def inp(m, ident):
    return next(o for o in m["inputs"] if o["id"] == ident)


def cycle(m):
    m["operators"] += [ring_op("n3"), ring_op("n5"), ring_op("n7")]
    m["edges"] += [edge("n3", "value", "n5", "value"), edge("n5", "value", "n7", "value"),
                   edge("n7", "value", "n3", "value")]


def self_loop(m):
    op(m, "thermal")["ports_in"][0]["arity"] = "2"
    m["edges"].append(edge("thermal", "value", "thermal", "value"))


def unknown_version(m):
    op(m, "thermal")["algorithm_version"] = "9.9.9"


def unknown_id(m):
    op(m, "fit")["algorithm_id"] = "fit.circle.mz"


def unsourced(m):
    del inp(m, "probe_r")["source"]


def tombstone(m):
    inp(m, "pts_noise")["source"]["digest"] = "sha256:" + "0" * 64


def empty_input_digest(m):
    inp(m, "form")["source"]["digest"] = "sha256:" + hashlib.sha256(b"").hexdigest()


def metre_to_radian(m):
    t = inp(m, "tilt")
    t["quantity_kind"], t["unit"] = "length", "si:m"


def radian_to_one(m):
    inp(m, "tilt")["unit"] = "si:1"


def kind_only(m):
    op(m, "fit")["ports_in"][1]["quantity_kind"] = "noise"


def arity_short(m):
    m["edges"] = [e for e in m["edges"] if e["from"]["node"] != "scale_err"]


def output_twice(m):
    m["edges"].append(edge("fit", "diameter", "d_bore", "value"))


def orphan_output(m):
    m["outputs"].append({"id": "orphan", "quantity_kind": "length", "unit": "si:m"})


def dangling_nodes(m):
    m["edges"].append(edge("ghost", "value", "ghost2", "value"))


def dangling_source_port(m):
    e = next(e for e in m["edges"] if e["to"]["node"] == "fit_residual")
    e["from"]["port"] = "resid"


def into_input(m):
    m["edges"].append(edge("probe_r", "value", "tilt", "value"))


def out_of_output(m):
    m["edges"].append(edge("d_bore", "value", "thermal", "value"))


def not_inputs(m):
    m["correlations"].append({"inputs": ["probe_r", "zz_ghost", "fit"], "matrix": mat([[1, 0, 0], [0, 1, 0], [0, 0, 1]])})


def indefinite(m):
    m["correlations"][1]["matrix"] = mat([[1, 0.9, 0.9], [0.9, 1, -0.9], [0.9, -0.9, 1]])


def asymmetric(m):
    m["correlations"][0]["matrix"] = mat([[1, 0.25], [0.3, 1]])


def diagonal(m):
    m["correlations"][0]["matrix"] = mat([[1, 0.25], [0.25, 0.5]])


def off_range(m):
    m["correlations"][0]["matrix"] = mat([[1, 1.5], [1.5, 1]])


def shuffled(m):
    """Same model, every list permuted and every object's members reversed."""
    s = copy.deepcopy(m)
    for k in ("inputs", "operators", "outputs"):
        s[k] = s[k][::-1]
    s["edges"] = schema.rotate(s["edges"], 3)[::-1]
    groups = []
    for g in s["correlations"][::-1]:
        n = len(g["inputs"])
        groups.append(schema.permute_group(g, list(range(n))[::-1]))
    s["correlations"] = groups
    return schema.reverse_keys(s)


def combine(*fs):
    def f(m):
        for g in fs:
            g(m)
    return f


ALL_FIVE = combine(cycle, unknown_version, unsourced, metre_to_radian, indefinite)
ALL_CODES = combine(cycle, unknown_version, unsourced, metre_to_radian, indefinite, arity_short, orphan_output,
                    dangling_nodes, not_inputs)

FIVE = [
    "cycle n3 n3,n5,n7",
    "unknown_operator thermal unc.op.scale@9.9.9",
    "unsourced_input probe_r",
    "unit_mismatch fit tilt tilt value",
    "invalid_correlation form indefinite",
]
EVERY = [
    "dangling_edge ghost2 value ghost value",
    "cycle n3 n3,n5,n7",
    "unknown_operator thermal unc.op.scale@9.9.9",
    "unsourced_input probe_r",
    "unit_mismatch fit tilt tilt value",
    "arity_mismatch orphan value 1 0",
    "arity_mismatch thermal strain 2 1",
    "unreachable_output orphan",
    "correlation_not_input fit fit",
    "correlation_not_input fit zz_ghost",
    "invalid_correlation form indefinite",
]

# (file, change, shuffled?, hand-labelled findings in order)
CASES = [
    ("cycle", cycle, False, ["cycle n3 n3,n5,n7"]),
    ("self_loop", self_loop, False, ["cycle thermal thermal"]),
    ("unknown_operator", unknown_version, False, ["unknown_operator thermal unc.op.scale@9.9.9"]),
    ("unknown_operator_id", unknown_id, False, ["unknown_operator fit fit.circle.mz@1.0.0"]),
    ("unsourced_input", unsourced, False, ["unsourced_input probe_r"]),
    ("source_tombstone", tombstone, False, ["unsourced_input pts_noise"]),
    ("source_empty_digest", empty_input_digest, False, ["unsourced_input form"]),
    ("unit_m_to_rad", metre_to_radian, False, ["unit_mismatch fit tilt tilt value"]),
    ("unit_rad_to_one", radian_to_one, False, ["unit_mismatch fit tilt tilt value"]),
    ("unit_kind", kind_only, False, ["unit_mismatch fit noise pts_noise value"]),
    ("arity_short", arity_short, False, ["arity_mismatch thermal strain 2 1"]),
    ("arity_output", output_twice, False, ["arity_mismatch d_bore value 1 2"]),
    ("unreachable_output", orphan_output, False, ["arity_mismatch orphan value 1 0", "unreachable_output orphan"]),
    ("dangling_node", dangling_nodes, False, ["dangling_edge ghost2 value ghost value"]),
    ("dangling_port", dangling_source_port, False,
     ["dangling_edge fit_residual value fit resid", "unreachable_output fit_residual"]),
    ("edge_into_input", into_input, False, ["dangling_edge tilt value probe_r value"]),
    ("edge_out_of_output", out_of_output, False,
     ["dangling_edge thermal value d_bore value", "arity_mismatch thermal value 1 2"]),
    ("correlation_not_input", not_inputs, False, ["correlation_not_input fit fit", "correlation_not_input fit zz_ghost"]),
    ("correlation_indefinite", indefinite, False, ["invalid_correlation form indefinite"]),
    ("correlation_asymmetric", asymmetric, False, ["invalid_correlation scale_err asymmetric"]),
    ("correlation_diagonal", diagonal, False, ["invalid_correlation scale_err diagonal"]),
    ("correlation_range", off_range, False, ["invalid_correlation scale_err off_diagonal_range"]),
    ("all_five", ALL_FIVE, False, FIVE),
    ("all_five_shuffled", ALL_FIVE, True, FIVE),
    ("all_codes", ALL_CODES, False, EVERY),
    ("all_codes_shuffled", ALL_CODES, True, EVERY),
]


def outputs():
    manifest = manifest_stub()
    base = schema.model()
    assert oracle(base, manifest) == [], "the base model must be valid"
    assert oracle(shuffled(base), manifest) == []
    files = {
        "manifest_stub.json": canon(manifest),
        "bore_ls.order.txt": ("\n".join(kahn_order(base)) + "\n").encode("ascii"),
    }
    rows = []
    for name, change, shuffle, labelled in CASES:
        m = copy.deepcopy(base)
        change(m)
        doc = shuffled(m) if shuffle else m
        got = oracle(doc, manifest)
        assert got == labelled, "%s: oracle %r, hand label %r" % (name, got, labelled)
        text = json.dumps(doc, indent="\t" if shuffle else 2, ensure_ascii=True) + "\n"
        files["bad/%s.json" % name] = text.encode("ascii")
        rows += ["%s.json\t%s" % (name, line) for line in labelled]
    files["bad/expected.txt"] = ("\n".join(rows) + "\n").encode("ascii")
    return files


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
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        on_disk = {"bad/" + p.name for p in BAD.glob("*") if p.is_file()}
        for extra in sorted(on_disk - set(files)):
            print("not generated: " + extra)
            bad += 1
    if bad:
        sys.exit(1)
    print("ok: %d files" % len(files))


if __name__ == "__main__":
    main()
