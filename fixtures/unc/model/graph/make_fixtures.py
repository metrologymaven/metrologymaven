#!/usr/bin/env python3
"""Independent reference for UNC.MODEL.GRAPH.SCHEMA fixtures (stdlib only; CC-BY-4.0 data).

A project-authored Uncertainty Model (bore diameter by least-squares circle fit, with a
thermal-strain correction) written in the `uncertainty-model/v1` document form pinned in
spec/unc/model/graph/schema/NODE.md and in the module documentation of fid-kunc `model`,
independently of the Rust code:

- canonical form: inputs, operators and outputs sorted by id; edges by (to.node, to.port,
  from.node, from.port); inside a correlation group the inputs sorted with the matrix rows
  and columns permuted to match; groups by first input id; port lists keep their order;
- canonical JSON: members sorted by key (all text is printable ASCII, so Python's
  code-point order equals RFC 8785's UTF-16 order), no whitespace, no escapes;
- every f64 is struct.pack('>d', x).hex(); arity is a decimal string;
- each operator embeds its full `params` object; params_digest = sha256 of the params'
  own canonical JSON, always written in the canonical form (the authored file supplies it,
  the shuffled file omits it, so both the checked and the derived path are exercised);
- digest = hashlib.sha256 of the canonical bytes, written as sha256:<hex>.

Files:
  bore_ls.json              the model as authored (indented, authored order)
  bore_ls.canon.json        its canonical bytes (no trailing newline)
  bore_ls.sha256            sha256:<hex> of bore_ls.canon.json (AC-1)
  bore_ls_shuffled.json     same model: nodes, edges, groups, group inputs and members permuted (AC-2)
  bore_ls_param.json        one embedded operator parameter changed (fit max_iter 50 -> 51) (AC-3)
  bore_ls_param.sha256      its digest
  dup_node.json             an output reuses an input id (AC-4: duplicate_node)
  bad_name.json             name with an upper-case letter (AC-4: schema_invalid{name})
  invalid/<case>.json       each breaks one rule (hand-labelled below)
  invalid/expected.txt      "<file> <code> <field, or the id for duplicate_node, or ->"

The 17 MB file of AC-4 is built by the Rust test at run time (not committed).

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import copy
import hashlib
import json
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent


def h(x):
    """IEEE-754 binary64 bit pattern, 16 lowercase hex digits."""
    return struct.pack(">d", x).hex()


def d(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def canon_bytes(obj):
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert "\\" not in text, "canonical models never need escapes"
    return text.encode("ascii")


def params_digest(params):
    """Digest of an operator's full parameter set (its own canonical JSON)."""
    return "sha256:" + hashlib.sha256(canon_bytes(params)).hexdigest()


FIT_PARAMS = {"max_iter": "50", "outlier": {"policy": "none", "k": "3"}, "tolerance": h(1e-12), "weights": []}
SCALE_PARAMS = {"mode": "multiplicative"}


def inp(ident, kind, unit, dist, params, src_kind, src_label, author):
    return {
        "id": ident,
        "quantity_kind": kind,
        "unit": unit,
        "distribution": {"kind": dist, "params": {k: h(v) for k, v in params.items()}},
        "source": {"kind": src_kind, "digest": d(src_label), "author": author},
    }


def port_in(name, kind, unit, arity):
    return {"name": name, "quantity_kind": kind, "unit": unit, "arity": str(arity)}


def port_out(name, kind, unit):
    return {"name": name, "quantity_kind": kind, "unit": unit}


def edge(fn, fp, tn, tp):
    return {"from": {"node": fn, "port": fp}, "to": {"node": tn, "port": tp}}


def model(fit_params=FIT_PARAMS):
    """The authored model (authored order, not canonical)."""
    return {
        "schema": "uncertainty-model/v1",
        "name": "bore-ls",
        "semver": "1.0.0",
        "inputs": [
            inp("probe_r", "length", "si:m", "normal", {"mu": 0.001, "sigma": 0.3e-6}, "dcc", "dcc:probe-2mm", "Cal Lab A"),
            inp("pts_noise", "length", "si:m", "scaled_t", {"mu": 0.0, "s": 0.8e-6, "nu": 9.0},
                "type_a_dataset", "typea:bore-ls-repeat", "metrology-maven fixtures"),
            inp("form", "length", "si:m", "trap", {"a": -1e-6, "b": 1e-6, "beta": 0.5},
                "qualification", "qual:ring-form", "QA team"),
            inp("thermal_strain", "strain", "si:1", "rect", {"a": -2.3e-6, "b": 2.3e-6}, "declared", "decl:thermal", "J. Doe"),
            inp("scale_err", "strain", "si:1", "tri", {"a": -1e-6, "b": 1e-6, "c": 0.0},
                "certificate", "cert:scale-2026", "Scale Cal GmbH"),
            inp("tilt", "angle", "si:rad", "arcsine", {"a": -1e-4, "b": 1e-4}, "qualification", "qual:tilt", "QA team"),
        ],
        "operators": [
            {
                "id": "fit",
                "algorithm_id": "fit.circle.ls",
                "algorithm_version": "1.0.0",
                "params": fit_params,
                "params_digest": params_digest(fit_params),
                "ports_in": [
                    port_in("probe_r", "length", "si:m", 1),
                    port_in("noise", "length", "si:m", 1),
                    port_in("form", "length", "si:m", 1),
                    port_in("tilt", "angle", "si:rad", 1),
                ],
                "ports_out": [port_out("diameter", "length", "si:m"), port_out("residual", "length", "si:m")],
            },
            {
                "id": "thermal",
                "algorithm_id": "unc.op.scale",
                "algorithm_version": "1.0.0",
                "params": SCALE_PARAMS,
                "params_digest": params_digest(SCALE_PARAMS),
                "ports_in": [port_in("value", "length", "si:m", 1), port_in("strain", "strain", "si:1", 2)],
                "ports_out": [port_out("value", "length", "si:m")],
            },
        ],
        "outputs": [
            {"id": "d_bore", "quantity_kind": "length", "unit": "si:m", "characteristic": "DIM-1"},
            {"id": "fit_residual", "quantity_kind": "length", "unit": "si:m"},
        ],
        "edges": [
            edge("probe_r", "value", "fit", "probe_r"),
            edge("pts_noise", "value", "fit", "noise"),
            edge("form", "value", "fit", "form"),
            edge("tilt", "value", "fit", "tilt"),
            edge("fit", "diameter", "thermal", "value"),
            edge("thermal_strain", "value", "thermal", "strain"),
            edge("scale_err", "value", "thermal", "strain"),
            edge("thermal", "value", "d_bore", "value"),
            edge("fit", "residual", "fit_residual", "value"),
        ],
        "correlations": [
            {"inputs": ["scale_err", "thermal_strain"], "matrix": mat([[1, 0.25], [0.25, 1]])},
            {"inputs": ["tilt", "form", "pts_noise"], "matrix": mat([[1, 0.1, 0.2], [0.1, 1, 0.3], [0.2, 0.3, 1]])},
        ],
    }


def mat(rows):
    return [[h(float(x)) for x in r] for r in rows]


def permute_group(g, order):
    """Group with inputs reordered to inputs[order[i]], matrix permuted to match."""
    return {
        "inputs": [g["inputs"][i] for i in order],
        "matrix": [[g["matrix"][i][j] for j in order] for i in order],
    }


def canonical(m):
    c = copy.deepcopy(m)
    for op in c["operators"]:
        op["params_digest"] = params_digest(op["params"])
    for k in ("inputs", "operators", "outputs"):
        c[k].sort(key=lambda n: n["id"])
    c["edges"].sort(key=lambda e: (e["to"]["node"], e["to"]["port"], e["from"]["node"], e["from"]["port"]))
    groups = []
    for g in c["correlations"]:
        order = sorted(range(len(g["inputs"])), key=lambda i: g["inputs"][i])
        groups.append(permute_group(g, order))
    groups.sort(key=lambda g: g["inputs"][0])
    c["correlations"] = groups
    return c


def reverse_keys(o):
    if isinstance(o, dict):
        return {k: reverse_keys(o[k]) for k in reversed(list(o))}
    if isinstance(o, list):
        return [reverse_keys(x) for x in o]
    return o


def rotate(xs, n):
    n %= len(xs)
    return xs[n:] + xs[:n]


def shuffled(m):
    s = copy.deepcopy(m)
    s["inputs"] = rotate(s["inputs"][::-1], 2)
    s["operators"] = s["operators"][::-1]
    for op in s["operators"]:
        del op["params_digest"]
    s["outputs"] = s["outputs"][::-1]
    s["edges"] = rotate(s["edges"], 4)[::-1]
    s["correlations"] = [permute_group(s["correlations"][1], [2, 0, 1]), permute_group(s["correlations"][0], [1, 0])]
    return reverse_keys(s)


def authored(m):
    return (json.dumps(m, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def mutated(f):
    m = model()
    f(m)
    return authored(m)


def set_in(path, value):
    def f(m):
        o = m
        for k in path[:-1]:
            o = o[k]
        o[path[-1]] = value

    return f


def del_in(path):
    def f(m):
        o = m
        for k in path[:-1]:
            o = o[k]
        del o[path[-1]]

    return f


def raw_author(raw):
    """The authored model with input 0's author replaced by raw bytes (not JSON-encoded)."""
    text = authored(model())
    marker = b'"author": "Cal Lab A"'
    assert text.count(marker) == 1
    return text.replace(marker, b'"author": "' + raw + b'"')


def nested(depth):
    v = "x"
    for _ in range(depth):
        v = [v]
    return v


def dup_port(m):
    m["operators"][0]["ports_in"].append(port_in("probe_r", "length", "si:m", 1))


def dup_edge(m):
    m["edges"].append(copy.deepcopy(m["edges"][3]))


def in_two_groups(m):
    m["correlations"][0] = {"inputs": ["scale_err", "thermal_strain", "tilt"], "matrix": mat([[1, 0, 0], [0, 1, 0], [0, 0, 1]])}


def dup_node_bad_copy(m):
    bad = copy.deepcopy(m["inputs"][2])
    bad["quantity_kind"] = "Bad"
    m["inputs"].insert(0, bad)


def two_groups_and_bad_matrix(m):
    in_two_groups(m)
    m["correlations"][1]["matrix"] = mat([[1, 0.1], [0.1, 1]])


def dup_node_inputs(m):
    m["inputs"].append(copy.deepcopy(m["inputs"][2]))


def dup_key_text():
    text = authored(model())
    marker = b'"semver": "1.0.0",'
    assert text.count(marker) == 1
    return text.replace(marker, marker + b' "semver": "1.0.0",')


# (file, bytes, code, field-or-id) -- hand-labelled from the rules in the module documentation.
INVALID = [
    ("unknown_member_root", mutated(set_in(["extra"], "x")), "schema_invalid", "model"),
    ("unknown_member_input", mutated(set_in(["inputs", 0, "note"], "x")), "schema_invalid", "inputs"),
    ("missing_semver", mutated(del_in(["semver"])), "schema_invalid", "semver"),
    ("semver_prerelease", mutated(set_in(["semver"], "1.0.0-rc.1")), "schema_invalid", "semver"),
    ("schema_v2", mutated(set_in(["schema"], "uncertainty-model/v2")), "schema_invalid", "schema"),
    ("unit_mm", mutated(set_in(["inputs", 0, "unit"], "mm")), "schema_invalid", "inputs.unit"),
    ("hex_upper", mutated(set_in(["inputs", 0, "distribution", "params", "sigma"], h(0.3e-6).upper())),
     "schema_invalid", "inputs.distribution.params.sigma"),
    ("neg_zero", mutated(set_in(["inputs", 1, "distribution", "params", "mu"], "8000000000000000")),
     "schema_invalid", "inputs.distribution.params.mu"),
    ("nan", mutated(set_in(["inputs", 0, "distribution", "params", "sigma"], "7ff8000000000000")),
     "schema_invalid", "inputs.distribution.params.sigma"),
    ("sigma_zero", mutated(set_in(["inputs", 0, "distribution", "params", "sigma"], h(0.0))),
     "schema_invalid", "inputs.distribution.params.sigma"),
    ("rect_reversed", mutated(set_in(["inputs", 3, "distribution", "params", "b"], h(-3e-6))),
     "schema_invalid", "inputs.distribution.params.b"),
    ("dist_kind", mutated(set_in(["inputs", 0, "distribution", "kind"], "gauss")),
     "schema_invalid", "inputs.distribution.kind"),
    ("dist_extra_param", mutated(set_in(["inputs", 0, "distribution", "params", "nu"], h(3.0))),
     "schema_invalid", "inputs.distribution.params"),
    ("source_kind", mutated(set_in(["inputs", 1, "source", "kind"], "type_a")), "schema_invalid", "inputs.source.kind"),
    ("source_digest", mutated(set_in(["inputs", 1, "source", "digest"], "sha256:ABC")),
     "schema_invalid", "inputs.source.digest"),
    ("author_lead_space", mutated(set_in(["inputs", 0, "source", "author"], " Cal Lab A")),
     "schema_invalid", "inputs.source.author"),
    ("author_double_space", mutated(set_in(["inputs", 0, "source", "author"], "Cal  Lab A")),
     "schema_invalid", "inputs.source.author"),
    ("author_bidi_raw", raw_author("Cal \u202eLab A".encode("utf-8")), "schema_invalid", "json.text"),
    ("author_bidi_escape", raw_author(b"Cal \\u202eLab A"), "schema_invalid", "json.escape"),
    ("author_control", raw_author(b"Cal\tLab A"), "schema_invalid", "json.text"),
    ("author_non_nfc", raw_author("Cal Lab Ae\u0301".encode("utf-8")), "schema_invalid", "json.text"),
    ("arity_number", authored(model()).replace(b'"arity": "2"', b'"arity": 2'), "schema_invalid", "json.value"),
    ("arity_zero", mutated(set_in(["operators", 1, "ports_in", 1, "arity"], "0")),
     "schema_invalid", "operators.ports_in.arity"),
    ("dup_port", mutated(dup_port), "schema_invalid", "operators.ports_in"),
    ("algorithm_upper", mutated(set_in(["operators", 0, "algorithm_id"], "Fit.circle.ls")),
     "schema_invalid", "operators.algorithm"),
    ("node_id_upper", mutated(set_in(["inputs", 0, "id"], "Probe_r")), "schema_invalid", "inputs.id"),
    ("quantity_kind", mutated(set_in(["outputs", 1, "quantity_kind"], "Length")),
     "schema_invalid", "outputs.quantity_kind"),
    ("characteristic_space", mutated(set_in(["outputs", 0, "characteristic"], "DIM 1")),
     "schema_invalid", "outputs.characteristic"),
    ("dup_node_inputs", mutated(dup_node_inputs), "duplicate_node", "form"),
    ("dup_node_bad_copy", mutated(dup_node_bad_copy), "duplicate_node", "form"),
    ("params_digest_mismatch", mutated(set_in(["operators", 0, "params_digest"], params_digest(SCALE_PARAMS))),
     "schema_invalid", "operators.params_digest"),
    ("params_digest_bad", mutated(set_in(["operators", 0, "params_digest"], "sha256:00")),
     "schema_invalid", "operators.params_digest"),
    ("params_missing", mutated(del_in(["operators", 0, "params"])), "schema_invalid", "operators.params"),
    ("params_not_object", mutated(set_in(["operators", 0, "params"], ["50"])), "schema_invalid", "operators.params"),
    ("params_number", authored(model()).replace(b'"max_iter": "50"', b'"max_iter": 50'), "schema_invalid", "json.value"),
    ("params_escape", authored(model()).replace(b'"policy": "none"', b'"policy": "n\\u006fne"'),
     "schema_invalid", "json.escape"),
    ("algorithm_id_65", mutated(set_in(["operators", 0, "algorithm_id"], "fit.circle." + "l" * 54)),
     "schema_invalid", "operators.algorithm_id"),
    ("dup_edge", mutated(dup_edge), "schema_invalid", "edges"),
    ("edge_port_upper", mutated(set_in(["edges", 0, "to", "port"], "Probe_r")), "schema_invalid", "edges.to.port"),
    ("group_single", mutated(set_in(["correlations", 0], {"inputs": ["scale_err"], "matrix": mat([[1]])})),
     "schema_invalid", "correlations.inputs"),
    ("group_matrix_shape", mutated(set_in(["correlations", 0, "matrix"], mat([[1, 0.25, 0], [0.25, 1, 0]]))),
     "schema_invalid", "correlations.matrix"),
    ("group_repeat_input", mutated(set_in(["correlations", 0, "inputs"], ["scale_err", "scale_err"])),
     "schema_invalid", "correlations.inputs"),
    ("input_in_two_groups", mutated(in_two_groups), "schema_invalid", "correlations.inputs"),
    ("input_in_two_groups_bad_matrix", mutated(two_groups_and_bad_matrix), "schema_invalid", "correlations.inputs"),
    ("duplicate_key", dup_key_text(), "schema_invalid", "json.duplicate_key"),
    ("trailing_data", authored(model()) + b"{}", "schema_invalid", "json.trailing_data"),
    ("bom", b"\xef\xbb\xbf" + authored(model()), "schema_invalid", "json.value"),
    ("empty", b"", "schema_invalid", "json.syntax"),
    ("depth_33", mutated(set_in(["inputs", 0, "note"], nested(30))), "too_large", "-"),
    ("depth_32_unknown", mutated(set_in(["inputs", 0, "note"], nested(29))), "schema_invalid", "inputs"),
]


def outputs():
    m = model()
    c = canon_bytes(canonical(m))
    p = canon_bytes(canonical(model(dict(FIT_PARAMS, max_iter="51"))))
    files = {
        "bore_ls.json": authored(m),
        "bore_ls.canon.json": c,
        "bore_ls.sha256": ("sha256:" + hashlib.sha256(c).hexdigest() + "\n").encode("ascii"),
        "bore_ls_shuffled.json": (json.dumps(shuffled(m), indent="\t", ensure_ascii=True) + "\n").encode("ascii"),
        "bore_ls_param.json": authored(model(dict(FIT_PARAMS, max_iter="51"))),
        "bore_ls_param.sha256": ("sha256:" + hashlib.sha256(p).hexdigest() + "\n").encode("ascii"),
        "dup_node.json": mutated(set_in(["outputs", 1, "id"], "probe_r")),
        "bad_name.json": mutated(set_in(["name"], "Bore-LS")),
    }
    # Self-checks of the oracle: the shuffle really permutes, and canonicalising it gives the same bytes.
    assert files["bore_ls_shuffled.json"] != files["bore_ls.json"]
    assert canon_bytes(canonical(json.loads(files["bore_ls_shuffled.json"]))) == c
    assert p != c
    rows = []
    for name, data, code, field in INVALID:
        files["invalid/" + name + ".json"] = data
        rows.append("%s.json %s %s" % (name, code, field))
    files["invalid/expected.txt"] = ("\n".join(sorted(rows)) + "\n").encode("ascii")
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
        on_disk = {p.relative_to(HERE).as_posix() for p in HERE.rglob("*") if p.is_file() and p.suffix in (".json", ".txt", ".sha256")}
        for extra in sorted(on_disk - set(files)):
            print("not generated: " + extra)
            bad += 1
    if bad:
        sys.exit(1)
    print("ok: %d files" % len(files))


if __name__ == "__main__":
    main()
