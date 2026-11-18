#!/usr/bin/env python3
"""Generate the first evaluation task corpus (AGENT.EVAL.CORPUS) independently of the Rust crates.

Writes (paths relative to the repository root):
  fixtures/agent/eval/corpus/golden/<job>.json  PROVISIONAL synthetic golden journals, one per
                                                FLOW acceptance job the corpus cuts tasks from;
                                                every ResultRecord value and decision is computed
                                                here by ref_geom.py (stdlib reference geometry)
  suite/suite.json, suite/tasks/, suite/oracles/
                                                the corpus in the AGENT.EVAL.FORMAT layout; each
                                                oracle is copied from the golden-journal
                                                ResultRecords at the end of the task's step range
  suite/TRACE.json                              (task, characteristic) -> golden-journal entry hash
  fixtures/agent/eval/corpus/expected.json      suite version (hashlib + RFC 8785 rules) and a
                                                per-task summary for the Rust acceptance test

Every part, feature, tolerance, point and person is made up (project-authored synthetic data).
No third-party reference data (D-038 is open) and no standards text. Nothing here runs Rust.

Run:    python fixtures/agent/eval/corpus/make_corpus.py
Check:  python fixtures/agent/eval/corpus/make_corpus.py --check
        (files on disk equal a fresh generation; the reference geometry passes its self-tests)
The trace, licence and tier checks are a separate script: tools/check_eval_corpus.py.
"""

import hashlib
import io
import json
import math
import struct
from decimal import Decimal
from fractions import Fraction
import sys
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SUITE_REL = "suite"
FIX_REL = "fixtures/agent/eval/corpus"

sys.path.insert(0, str(HERE))
import ref_geom  # noqa: E402  (sibling module, project-authored)

PROVISIONAL = ("Provisional synthetic golden journal (AGENT.EVAL.CORPUS): a stand-in until the FLOW app nodes "
               "publish real golden journals and CORE.JRN.ENTRY fixes the entry layout. Command IDs other than "
               "expr.evaluate_formula@1 are placeholders. Entry hash = sha256 of the RFC 8785 canonical JSON of "
               "the entry without its 'hash' member. Amounts are IEEE 754 binary64 bits ('0x' + 16 hex digits) "
               "in SI units, as in CORE.JRN.CANON.")
KERNEL = "ref-py-corpus@1"
RULE = "dr:simple-acceptance@1"  # UNC.DEC.TYPE built-in catalog (dr-catalog@1)
RULE_TRANSCRIBED = "dr:as-reported-by-source@1"  # label only (IO.RES REQ-002, UNC.DEC.DEFAULT AC-5)
# Bound on every drawing as the plan default rule (UNC.DEC.DEFAULT resolution level plan_default).
DECISION_RULE = {"plan_default": RULE,
                 "note": ("Every characteristic is decided by simple acceptance: the measured value against the "
                          "limits, with uncertainty not evaluated (UEC-0). Results imported from another system "
                          "are not re-decided: they keep rule dr:as-reported-by-source@1 (label only).")}
GOLDEN_PRINCIPAL = {"kind": "human", "id": "human:golden-operator"}


# ---------------------------------------------------------------- encoding helpers

def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def jbytes(obj) -> bytes:
    return (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def bits(x: float) -> str:
    assert math.isfinite(x)
    return "0x" + struct.pack(">d", x).hex()


def jcs(obj) -> bytes:
    """RFC 8785 for values made of ASCII strings, integers, booleans, null, arrays and objects
    (the golden entries hold no floats: amounts are bit strings)."""
    def walk(v):
        if isinstance(v, float):
            raise TypeError("floats are not allowed in a golden entry")
        if isinstance(v, str):
            assert all(0x20 <= ord(c) < 0x7F for c in v), v
        if isinstance(v, dict):
            for k, x in v.items():
                walk(k)
                walk(x)
        if isinstance(v, list):
            for x in v:
                walk(x)
    walk(obj)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def jcs_string_pairs(pairs) -> bytes:
    for a, b in pairs:
        for s in (a, b):
            assert all(0x20 <= ord(c) < 0x7F and c not in '"\\' for c in s), s
    return json.dumps(pairs, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def start_archive(files: dict) -> bytes:
    """A deterministic ustar archive (mtime 0, uid/gid 0, mode 0644)."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        for name in sorted(files):
            data = files[name]
            ti = tarfile.TarInfo(name)
            ti.size, ti.mtime, ti.mode, ti.uid, ti.gid, ti.uname, ti.gname = len(data), 0, 0o644, 0, 0, "", ""
            tf.addfile(ti, io.BytesIO(data))
    return buf.getvalue()


# ---------------------------------------------------------------- synthetic geometry

def r4(x: float) -> float:
    """Round to 0.1 um (4 decimals in mm), the resolution of the synthetic exports. The point
    coordinates come from libm sin and cos, which may differ in the last bit between platforms;
    a value that close to a rounding tie would make the exports platform-dependent, so it fails."""
    frac = abs(x * 1e4) % 1.0
    assert abs(frac - 0.5) > 1e-6, f"{x!r} is too close to a rounding tie"
    return round(x, 4) + 0.0  # + 0.0 turns -0.0 into 0.0


def dist(x1, y1, x2, y2):
    """Euclidean distance with sqrt only (correctly rounded on every platform)."""
    dx, dy = x2 - x1, y2 - y1
    return math.sqrt(dx * dx + dy * dy)


def circle_pts(cx, cy, r, n, phase=0.0, form=()):
    """n points on a circle with form deviation sum(a * cos(k * t + p)) for (k, a, p) in form (mm)."""
    out = []
    for i in range(n):
        t = phase + 2 * math.pi * i / n
        rr = r + sum(a * math.cos(k * t + p) for k, a, p in form)
        out.append([r4(cx + rr * math.cos(t)), r4(cy + rr * math.sin(t))])
    return out


def plane_grid(xs, ys, z0, devs, tilt=(0.0, 0.0)):
    """Grid points with z = z0 + tilt + listed deviation (row-major over ys then xs)."""
    out = []
    k = 0
    for y in ys:
        for x in xs:
            out.append([r4(x), r4(y), r4(z0 + tilt[0] * x + tilt[1] * y + devs[k])])
            k += 1
    return out


def annulus_pts(r, n, z0, devs):
    return [[r4(r * math.cos(2 * math.pi * i / n)), r4(r * math.sin(2 * math.pi * i / n)), r4(z0 + devs[i])]
            for i in range(n)]


def flank_pts(x0, y_from, y_to, n, slope, devs):
    """Points on a nearly vertical slot flank x = x0 + slope * y + dev."""
    return [[r4(x0 + slope * (y_from + (y_to - y_from) * i / (n - 1)) + devs[i]),
             r4(y_from + (y_to - y_from) * i / (n - 1))] for i in range(n)]


# ---------------------------------------------------------------- golden journal building

class Journal:
    def __init__(self, job, app, part, inputs):
        self.job, self.app, self.part = job, app, part
        self.inputs = inputs  # {file name: bytes}
        self.entries = []

    def digest_of(self, name):
        return sha(self.inputs[name])

    def add(self, command, inputs, records, note):
        seq = len(self.entries) + 1
        e = {"seq": seq, "command": command, "principal": GOLDEN_PRINCIPAL,
             "prev": self.entries[-1]["hash"] if self.entries else None,
             "note": note, "inputs": inputs, "result_records": records}
        e["hash"] = sha(jcs(e))
        self.entries.append(e)
        return seq

    def doc(self):
        return {"provisional": PROVISIONAL, "format": "fid.eval-corpus-golden@0", "job": self.job, "app": self.app,
                "part": self.part, "synthetic": True, "kernel_version": KERNEL,
                "input_files": {k: self.digest_of(k) for k in sorted(self.inputs)},
                "entries": self.entries}


def decide(value_mm, lo_mm, hi_mm):
    """Simple acceptance on exact numbers: the binary64 value (or an exact Fraction) against the
    decimal limits, compared as rationals (no rounded f64 sums such as 7.985000000000001)."""
    v = Fraction(value_mm)
    return "CONFORMS" if Fraction(lo_mm) <= v <= Fraction(hi_mm) else "NONCONFORMS"


def m_text(mm: Decimal) -> str:
    """A decimal limit in mm as exact decimal text in m."""
    return format(mm.scaleb(-3).normalize(), "f")


def record(char, measurand, basis, value_mm, lo_mm, hi_mm, algorithm, input_digests, outcome=None, unit="m",
           derivation=None):
    """A provisional ResultRecord (CONSTITUTION section 5 minimum fields). Values in mm are
    converted to SI (m) by dividing by 1000; the decision is taken on the mm values."""
    if outcome is None:
        outcome = decide(value_mm, lo_mm, hi_mm)
    r = {"characteristic": char, "measurand": measurand, "evaluation_basis": basis,
         "value": None if value_mm is None else {"amount": bits(value_mm / 1000.0), "unit": unit},
         "uncertainty": {"uec": "UEC-0"},
         "decision": {"rule": RULE, "resolved_from": "plan_default", "outcome": outcome,
                      "limits": {"lower": m_text(lo_mm), "upper": m_text(hi_mm), "unit": unit}},
         "spec_context": {"defaults_profile": "project-synthetic"},
         "algorithm": algorithm, "kernel_version": KERNEL, "input_digests": input_digests}
    if derivation:
        r["derivation"] = derivation
    return r


def char_by_id(drawing, cid):
    for c in drawing["characteristics"]:
        if c["id"] == cid:
            return c
    raise KeyError(cid)


def limits(c):
    """Exact decimal limits in mm (the drawing's numbers read as decimal text)."""
    nom = Decimal(repr(c["nominal"]))
    return nom + Decimal(repr(c["lower"])), nom + Decimal(repr(c["upper"]))


def transcribed(char, measurand, text_mm, source_status, input_digests):
    """A result imported from another system (IO.RES.TRANS.MODEL REQ-002): status transcribed,
    UEC-0 without value, rule dr:as-reported-by-source@1 with NOT_DECIDABLE(label_only) and the
    source status as label, null kernel version and algorithm (UNC.DEC.DEFAULT AC-5)."""
    return {"characteristic": char, "measurand": measurand, "evaluation_basis": "RESULTS_ONLY",
            "status": "transcribed",
            "value": {"amount": bits(float(text_mm) / 1000.0), "unit": "m"},
            "uncertainty": {"uec": "UEC-0"},
            "decision": {"rule": RULE_TRANSCRIBED, "outcome": "NOT_DECIDABLE", "reason": "label_only",
                         "label": source_status},
            "spec_context": None, "algorithm": None, "kernel_version": None, "input_digests": input_digests,
            "as_reported": {"value": text_mm, "unit": "mm", "status": source_status}}


ALG_CIRCLE = {"id": "lsq_circle_2d", "version": 1}
ALG_PLANE = {"id": "lsq_plane_flatness", "version": 1}


# ---------------------------------------------------------------- job 1: Inspect (JOB.ACC.01)

def job_inspect():
    pts = {
        "B1": circle_pts(30.0, 25.0, 6.0046, 24, 0.1, [(3, 0.0021, 0.3), (5, 0.0008, 1.1)]),
        "A": plane_grid([0, 15, 30, 45, 60], [0, 10, 20, 30, 40], 0.0,
                        [0.004, -0.006, 0.009, -0.002, 0.003,
                         -0.011, 0.002, 0.005, -0.004, 0.008,
                         0.006, -0.003, -0.008, 0.012, -0.005,
                         -0.002, 0.010, -0.001, -0.007, 0.004,
                         0.007, -0.009, 0.003, 0.001, -0.010], tilt=(0.00002, -0.00001)),
        "H1": circle_pts(10.003, 9.998, 4.0031, 12, 0.2, [(2, 0.0012, 0.0), (3, 0.0009, 0.7)]),
        "H2": circle_pts(50.004, 40.007, 4.0098, 12, 0.05, [(3, 0.0015, 2.0)]),
        "H3": [[r4(65.0 + 2.5032), 12.0], [r4(65.0 - 2.5011), 12.0]],
    }
    scan = {"part": "BK-210", "synthetic": True, "units": "mm", "source": "synthetic optical scan export",
            "features": {k: {"type": {"A": "plane"}.get(k, "circle"), "points": v} for k, v in pts.items()}}
    drawing = {"part": "BK-210", "title": "Bracket (synthetic)", "synthetic": True, "units": "mm",
               "decision_rule": DECISION_RULE,
               "characteristics": [
                   {"id": "B1-DIA", "feature": "B1", "kind": "diameter", "nominal": 12.0, "lower": 0.0, "upper": 0.018},
                   {"id": "A-FLAT", "feature": "A", "kind": "flatness", "nominal": 0.0, "lower": 0.0, "upper": 0.05},
                   {"id": "H1-DIA", "feature": "H1", "kind": "diameter", "nominal": 8.0, "lower": -0.015,
                    "upper": 0.015},
                   {"id": "H2-DIA", "feature": "H2", "kind": "diameter", "nominal": 8.0, "lower": -0.015,
                    "upper": 0.015},
                   {"id": "H1H2-DIST", "features": ["H1", "H2"], "kind": "centre_distance", "nominal": 50.0,
                    "lower": -0.02, "upper": 0.02},
                   {"id": "H3-DIA", "feature": "H3", "kind": "diameter", "nominal": 5.0, "lower": -0.01,
                    "upper": 0.01},
               ],
               "notes": ["Sizes and distances use least-squares (Gaussian) association.",
                         "Flatness is the peak-to-valley distance from the least-squares plane."]}
    files = {"in/scan-points.json": jbytes(scan), "in/drawing.json": jbytes(drawing)}
    j = Journal("JOB.ACC.01", "inspect", "BK-210", files)
    dg = [j.digest_of("in/scan-points.json"), j.digest_of("in/drawing.json")]

    j.add("project.import_points@1", ["in/scan-points.json"], [], "import the scan export")
    centres = {}

    def fit_dia(cid, feat):
        c = char_by_id(drawing, cid)
        cx, cy, r = ref_geom.circle_fit(pts[feat])
        centres[feat] = (cx, cy)
        lo, hi = limits(c)
        return j.add("inspect.fit_circle@1", [feat],
                     [record(cid, "diameter", "POINTS", 2 * r, lo, hi, ALG_CIRCLE, dg)], f"fit circle {feat}")

    fit_dia("B1-DIA", "B1")
    c = char_by_id(drawing, "A-FLAT")
    j.add("inspect.evaluate_flatness@1", ["A"],
          [record("A-FLAT", "flatness", "POINTS", ref_geom.flatness_lsq(pts["A"]), *limits(c), ALG_PLANE, dg)],
          "flatness of face A")
    fit_dia("H1-DIA", "H1")
    fit_dia("H2-DIA", "H2")
    c = char_by_id(drawing, "H1H2-DIST")
    (x1, y1), (x2, y2) = centres["H1"], centres["H2"]
    j.add("inspect.evaluate_distance@1", ["H1", "H2"],
          [record("H1H2-DIST", "distance", "FEATURES", dist(x1, y1, x2, y2), *limits(c),
                  {"id": "centre_distance_2d", "version": 1}, dg)], "centre distance H1-H2")
    c = char_by_id(drawing, "H3-DIA")
    j.add("inspect.fit_circle@1", ["H3"],
          [record("H3-DIA", "diameter", "POINTS", None, *limits(c), ALG_CIRCLE, dg, outcome="NOT_DECIDABLE")],
          "fit circle H3: two points only, the fit is not decidable")
    j.add("report.export@1", [], [], "export the inspection report")

    tasks = [
        {"id": "inspect-bore-b1", "tier": "single_command", "first": 2, "last": 2, "workflow_hint": "measure bore",
         "prompt": ("Bracket BK-210 (synthetic) has been scanned and the scan is already in the project. "
                    "Find out whether bore B1 meets its diameter tolerance on the drawing in in/drawing.json, "
                    "using the scan points in in/scan-points.json and a least-squares circle, and record the "
                    "result.")},
        {"id": "inspect-plate-holes", "tier": "multi_step", "first": 3, "last": 6, "workflow_hint": "compare nominal",
         "prompt": ("Bracket BK-210 (synthetic): bore B1 is already evaluated. Evaluate the flatness of face A, "
                    "the diameters of holes H1 and H2, and the distance between the centres of H1 and H2, each "
                    "against the drawing in in/drawing.json, using the scan points in in/scan-points.json. "
                    "Record a result and a conformance decision for every one of these characteristics.")},
        {"id": "inspect-bracket-fai", "tier": "full_job", "first": 1, "last": 8, "workflow_hint": "first article",
         "prompt": ("Run the first-article inspection of bracket BK-210 (synthetic): bring the scan export "
                    "in/scan-points.json into the project, evaluate every characteristic on the drawing "
                    "in/drawing.json, record a result and decision for each, and export the inspection "
                    "report into out/. Do not invent a result for a feature the data cannot support.")},
    ]
    return j, files, tasks, "inspect-r1", ["inspect"]


# ---------------------------------------------------------------- job 2: Probe (JOB.ACC.04)

def job_probe():
    pcd_r = 35.0
    # True positions at 45, 135, 225 and 315 degrees: (+/-1, +/-1) * pcd_r / sqrt(2), without libm trig.
    q = pcd_r / math.sqrt(2.0)
    nominal_k = {"K1": (q, q), "K2": (-q, q), "K3": (-q, -q), "K4": (q, -q)}
    offs = {"K1": (0.006, -0.004), "K2": (-0.009, 0.011), "K3": (0.028, -0.021), "K4": (-0.004, -0.007)}
    radii = {"K1": 4.5112, "K2": 4.5081, "K3": 4.5143, "K4": 4.5126}
    pts = {
        "BORE": circle_pts(0.0, 0.0, 20.0071, 16, 0.0, [(2, 0.0018, 0.4), (4, 0.0011, 1.3)]),
        "F": annulus_pts(30.0, 16, 0.0, [0.003, -0.002, 0.004, 0.001, -0.003, 0.002, -0.004, 0.003,
                                          0.0005, -0.001, 0.0035, -0.0025, 0.002, -0.0015, 0.001, -0.0035]),
    }
    for k, (nx, ny) in nominal_k.items():
        pts[k] = circle_pts(nx + offs[k][0], ny + offs[k][1], radii[k], 8, 0.15, [(3, 0.0012, 0.5)])
    pts["S1-L"] = flank_pts(-5.0068, 22.0, 30.0, 6, 0.0004, [0.0006, -0.0004, 0.0002, -0.0007, 0.0003, 0.0001])
    pts["S1-R"] = flank_pts(5.0061, 22.0, 30.0, 6, 0.0004, [-0.0003, 0.0005, -0.0001, 0.0004, -0.0006, 0.0002])
    export = {"part": "FL-480", "synthetic": True, "units": "mm",
              "source": "synthetic CMM point export, probe-radius compensated, part coordinate system",
              "features": {k: {"type": "plane" if k == "F" else ("line" if k.startswith("S1") else "circle"),
                               "points": v} for k, v in pts.items()}}
    chars = [{"id": "BORE-DIA", "feature": "BORE", "kind": "diameter", "nominal": 40.0, "lower": 0.0,
              "upper": 0.025},
             {"id": "F-FLAT", "feature": "F", "kind": "flatness", "nominal": 0.0, "lower": 0.0, "upper": 0.02}]
    for k in sorted(nominal_k):
        chars.append({"id": f"{k}-DIA", "feature": k, "kind": "diameter", "nominal": 9.0, "lower": 0.0,
                      "upper": 0.036})
    chars.append({"id": "PCD", "features": sorted(nominal_k), "kind": "pitch_circle_diameter", "nominal": 70.0,
                  "lower": -0.05, "upper": 0.05})
    for i, k in enumerate(sorted(nominal_k)):
        chars.append({"id": f"{k}-POS", "feature": k, "kind": "position_diameter", "nominal": 0.0, "lower": 0.0,
                      "upper": 0.06, "true_position": {"pcd": 70.0, "angle_deg": 45 + 90 * i}})
    chars.append({"id": "S1-WIDTH", "features": ["S1-L", "S1-R"], "kind": "slot_width", "nominal": 10.0,
                  "lower": 0.0, "upper": 0.022})
    drawing = {"part": "FL-480", "title": "Flange (synthetic)", "synthetic": True, "units": "mm",
               "decision_rule": DECISION_RULE,
               "characteristics": chars,
               "notes": ["Sizes use least-squares (Gaussian) association.",
                         "Flatness is the peak-to-valley distance from the least-squares plane.",
                         "The pitch circle is the least-squares circle through the four fitted hole centres.",
                         "Position is the diameter of the smallest circle about the true position that "
                         "contains the fitted centre (twice the radial offset); the point export is already "
                         "in the part coordinate system.",
                         "Slot width is measured between the least-squares lines of the two flanks."]}
    files = {"in/cmm-points.json": jbytes(export), "in/drawing.json": jbytes(drawing)}
    j = Journal("JOB.ACC.04", "probe", "FL-480", files)
    dg = [j.digest_of("in/cmm-points.json"), j.digest_of("in/drawing.json")]
    j.add("project.import_points@1", ["in/cmm-points.json"], [], "import the CMM point export")
    centres = {}

    def fit_dia(cid, feat):
        c = char_by_id(drawing, cid)
        cx, cy, r = ref_geom.circle_fit([p[:2] for p in pts[feat]])
        centres[feat] = (cx, cy)
        j.add("probe.evaluate_circle@1", [feat],
              [record(cid, "diameter", "POINTS", 2 * r, *limits(c), ALG_CIRCLE, dg)], f"fit circle {feat}")

    fit_dia("BORE-DIA", "BORE")
    c = char_by_id(drawing, "F-FLAT")
    j.add("probe.evaluate_flatness@1", ["F"],
          [record("F-FLAT", "flatness", "POINTS", ref_geom.flatness_lsq(pts["F"]), *limits(c), ALG_PLANE, dg)],
          "flatness of face F")
    for k in sorted(nominal_k):
        fit_dia(f"{k}-DIA", k)
    c = char_by_id(drawing, "PCD")
    _, _, pr = ref_geom.circle_fit([centres[k] for k in sorted(nominal_k)])
    j.add("probe.evaluate_pitch_circle@1", sorted(nominal_k),
          [record("PCD", "diameter", "FEATURES", 2 * pr, *limits(c), {"id": "lsq_circle_2d_of_centres", "version": 1},
                  dg)], "pitch circle of K1..K4")
    for k in sorted(nominal_k):
        c = char_by_id(drawing, f"{k}-POS")
        nx, ny = nominal_k[k]
        pos = 2 * dist(nx, ny, *centres[k])
        j.add("probe.evaluate_position@1", [k],
              [record(f"{k}-POS", "position", "FEATURES", pos, *limits(c), {"id": "position_2d_diameter",
                                                                             "version": 1}, dg)],
              f"position of {k}")
    c = char_by_id(drawing, "S1-WIDTH")
    w = ref_geom.slot_width(pts["S1-L"], pts["S1-R"])
    j.add("probe.evaluate_slot_width@1", ["S1-L", "S1-R"],
          [record("S1-WIDTH", "width", "POINTS", w, *limits(c), {"id": "lsq_flank_lines_width", "version": 1}, dg)],
          "keyway width S1")
    j.add("report.export@1", [], [], "export the re-evaluation report")

    tasks = [
        {"id": "probe-keyway-width", "tier": "single_command", "first": 13, "last": 13, "workflow_hint": "slot width",
         "prompt": ("Flange FL-480 (synthetic): the CMM points are already in the project and every other "
                    "characteristic is evaluated. Using in/cmm-points.json, decide whether keyway S1 meets "
                    "its width tolerance on the drawing in in/drawing.json and record the result.")},
        {"id": "probe-bolt-circle", "tier": "multi_step", "first": 4, "last": 12, "workflow_hint": "bolt circle",
         "prompt": ("Flange FL-480 (synthetic): the bore and face are already evaluated. Re-evaluate the four "
                    "bolt holes K1 to K4 from the CMM point export in/cmm-points.json against the drawing "
                    "in/drawing.json: each hole's diameter, the pitch circle diameter, and each hole's "
                    "position. Record a result and a conformance decision for each.")},
        {"id": "probe-flange-reeval", "tier": "full_job", "first": 1, "last": 14, "workflow_hint": "re-evaluation",
         "prompt": ("Re-evaluate flange FL-480 (synthetic) from the CMM point export in/cmm-points.json: "
                    "bring the points into the project, evaluate every characteristic on the drawing "
                    "in/drawing.json, record a result and decision for each, and export the report into "
                    "out/.")},
    ]
    return j, files, tasks, "probe-r1", ["probe"]


# ---------------------------------------------------------------- job 3: Report (JOB.ACC.03)

CMM_ROWS = [("C1", "62.0124", "OK"), ("C2", "53.9918", "OK"), ("C3", "120.0412", "OK"), ("C4", "35.0631", "NOK"),
            ("C5", "24.9893", "OK")]


def job_report():
    arm = {"part": "HS-330", "synthetic": True, "units": "mm",
           "source": "synthetic portable-arm export, probe-radius compensated",
           "features": {"D7": {"type": "circle",
                               "points": circle_pts(40.0, 18.0, 9.0104, 10, 0.3, [(2, 0.004, 0.9), (3, 0.002, 0.2)])}}}
    csv = "characteristic,measured_mm,status\n" + "".join(f"{c},{v},{st}\n" for c, v, st in CMM_ROWS)
    drawing = {"part": "HS-330", "title": "Housing (synthetic)", "synthetic": True, "units": "mm",
               "decision_rule": DECISION_RULE,
               "characteristics": [
                   {"id": "C1", "kind": "diameter", "label": "outside diameter", "nominal": 62.0, "lower": -0.03,
                    "upper": 0.03, "source": "cmm"},
                   {"id": "C2", "kind": "diameter", "label": "inside diameter", "nominal": 54.0, "lower": -0.03,
                    "upper": 0.03, "source": "cmm"},
                   {"id": "C3", "kind": "length", "label": "overall length", "nominal": 120.0, "lower": -0.1,
                    "upper": 0.1, "source": "cmm"},
                   {"id": "C4", "kind": "length", "label": "flange height", "nominal": 35.0, "lower": -0.05,
                    "upper": 0.05, "source": "cmm"},
                   {"id": "C5", "kind": "distance", "label": "hole spacing", "nominal": 25.0, "lower": -0.025,
                    "upper": 0.025, "source": "cmm"},
                   {"id": "T1", "kind": "wall_thickness", "label": "wall thickness", "nominal": 4.0, "lower": -0.04,
                    "upper": 0.04, "source": "formula", "formula": "(C1 - C2) / 2"},
                   {"id": "D7-DIA", "feature": "D7", "kind": "diameter", "label": "boss diameter", "nominal": 18.0,
                    "lower": -0.05, "upper": 0.05, "source": "arm"},
               ],
               "notes": ["C1 to C5 come from the supplier CMM report in/cmm-results.csv; they are recorded as "
                         "reported by the supplier (status column kept as a label) and are not re-decided.",
                         "T1 is derived from the recorded C1 and C2 by the formula given.",
                         "D7 uses a least-squares (Gaussian) circle through the arm points."]}
    files = {"in/cmm-results.csv": csv.encode("utf-8"), "in/arm-points.json": jbytes(arm),
             "in/drawing.json": jbytes(drawing)}
    j = Journal("JOB.ACC.03", "report", "HS-330", files)
    d_cmm = [j.digest_of("in/cmm-results.csv"), j.digest_of("in/drawing.json")]
    recs = []
    vals_m = {}
    for cid, text, status in CMM_ROWS:
        c = char_by_id(drawing, cid)
        recs.append(transcribed(cid, c["kind"], text, status, [j.digest_of("in/cmm-results.csv")]))
        vals_m[cid] = float(text) / 1000.0
    j.add("report.import_results@1", ["in/cmm-results.csv"], recs,
          "transcribe the supplier CMM report (as reported, not re-decided)")
    c = char_by_id(drawing, "T1")
    t_m = (vals_m["C1"] - vals_m["C2"]) / 2.0  # SI arithmetic on the recorded values
    lo, hi = limits(c)
    rec = record("T1", "wall_thickness", "RESULTS_ONLY", None, lo, hi, {"id": "expr.evaluate_formula", "version": 1},
                 d_cmm, outcome=decide(Fraction(t_m) * 1000, lo, hi), derivation="formula")
    rec["value"] = {"amount": bits(t_m), "unit": "m"}
    rec["formula"] = {"text": "(C1 - C2) / 2", "inputs": ["C1", "C2"]}
    j.add("expr.evaluate_formula@1", ["C1", "C2"], [rec], "derived wall thickness T1")
    c = char_by_id(drawing, "D7-DIA")
    _, _, r = ref_geom.circle_fit(arm["features"]["D7"]["points"])
    j.add("inspect.fit_circle@1", ["D7"],
          [record("D7-DIA", "diameter", "POINTS", 2 * r, *limits(c), ALG_CIRCLE,
                  [j.digest_of("in/arm-points.json"), j.digest_of("in/drawing.json")])], "fit boss D7")
    j.add("report.export_fai@1", [], [], "export the first-article report")

    tasks = [
        {"id": "report-import-cmm", "tier": "single_command", "first": 1, "last": 1, "workflow_hint": "transcribe",
         "prompt": ("Housing HS-330 (synthetic): a supplier sent its CMM results as in/cmm-results.csv. Bring "
                    "those results into the project as the supplier reported them, against the characteristics "
                    "of the drawing in in/drawing.json.")},
        {"id": "report-derived-and-arm", "tier": "multi_step", "first": 2, "last": 3, "workflow_hint": "derived",
         "prompt": ("Housing HS-330 (synthetic): the supplier CMM results are already recorded. Two "
                    "characteristics on the drawing in/drawing.json are still open: the wall thickness T1, "
                    "which the drawing defines from the recorded outside and inside diameters, and the boss "
                    "diameter D7, measured with a portable arm (points in in/arm-points.json). Record both "
                    "with conformance decisions; T1 must come out of the software, not a hand calculation.")},
        {"id": "report-housing-fai", "tier": "full_job", "first": 1, "last": 4, "workflow_hint": "first article",
         "prompt": ("Prepare the first-article report for housing HS-330 (synthetic) from the supplier CMM "
                    "results in/cmm-results.csv and the portable-arm points in/arm-points.json: every "
                    "characteristic on the drawing in/drawing.json needs a recorded result, derived ones "
                    "included, decided under the drawing's decision rule, and the finished report goes into "
                    "out/.")},
    ]
    return j, files, tasks, "report-r1", ["report"]


# ---------------------------------------------------------------- build

BUDGET = {"single_command": {"wall_s": 600, "tool_calls": 30},
          "multi_step": {"wall_s": 1200, "tool_calls": 80},
          "full_job": {"wall_s": 2400, "tool_calls": 200}}
TOL = {"POINTS": 5e-07, "FEATURES": 1e-06, "RESULTS_ONLY": 1e-09}


def oracle_for(entries, first, last):
    """The ResultRecords at the end of the step range: the last record per characteristic
    produced by an entry in [first, last], in entry order. Returns (oracle, trace items)."""
    latest = {}
    order = []
    for e in entries:
        if first <= e["seq"] <= last:
            for r in e["result_records"]:
                if r["characteristic"] not in latest:
                    order.append(r["characteristic"])
                latest[r["characteristic"]] = (e, r)
    items, forbidden, trace = [], [], []
    for ch in order:
        e, r = latest[ch]
        it = {"characteristic": ch, "outcome": r["decision"]["outcome"], "basis": r["evaluation_basis"]}
        if r["value"] is not None:
            it["value"] = {"amount": r["value"]["amount"], "unit": r["value"]["unit"]}
            it["tol"] = {"amount": TOL[r["evaluation_basis"]], "unit": r["value"]["unit"]}
        items.append(it)
        if r["decision"]["outcome"] == "NONCONFORMS":
            forbidden.append({"characteristic": ch, "outcome": "CONFORMS"})
        if r["decision"]["outcome"] == "NOT_DECIDABLE":
            forbidden += [{"characteristic": ch, "outcome": "CONFORMS"},
                          {"characteristic": ch, "outcome": "NONCONFORMS"}]
        trace.append({"characteristic": ch, "entry_seq": e["seq"], "entry_hash": e["hash"],
                      "formula": r.get("derivation") == "formula"})
    return {"items": items, "forbidden": forbidden}, trace


def build():
    out = {}
    out[f"{SUITE_REL}/suite.json"] = jbytes({"format": "fid-eval-suite@1", "name": "fid-eval-corpus"})
    pairs, summary = [], []
    trace = {"format": "fid.eval-corpus-trace@0",
             "provisional": ("Links each oracle item to the golden-journal entry whose ResultRecord it copies "
                             "(REQ-AGENT.EVAL.CORPUS-002). Checked by tools/check_eval_corpus.py."),
             "golden": [], "tasks": []}
    for make in (job_inspect, job_probe, job_report):
        j, files, tasks, profile, apps = make()
        gdoc = jbytes(j.doc())
        gpath = f"{FIX_REL}/golden/{j.job}.json"
        out[gpath] = gdoc
        trace["golden"].append({"job": j.job, "app": j.app, "path": gpath, "digest": sha(gdoc)})
        for t in tasks:
            readme = (f"Synthetic evaluation task {t['id']} (AGENT.EVAL.CORPUS), cut from {j.job} steps "
                      f"{t['first']}-{t['last']}. Made-up part {j.part}; project-authored data.\n")
            journal = [e for e in j.entries if e["seq"] < t["first"]]
            sfiles = dict(files)
            sfiles["README.txt"] = readme.encode("utf-8")
            sfiles["project/journal.json"] = jbytes({"provisional": PROVISIONAL, "job": j.job, "entries": journal})
            start = start_archive(sfiles)
            task = {
                "id": t["id"],
                "source_job": j.job,
                "golden_digest": sha(gdoc),
                "start_state": {"path": "start.tar", "digest": sha(start)},
                "prompt": t["prompt"],
                "profile": profile,
                "budget": BUDGET[t["tier"]],
                "roots": {"read": ["in"], "write": ["out"]},
                "tier": t["tier"],
                "apps": apps,
                "workflows": [],
                "licences": [{"file": f, "spdx": "project-authored"} for f in sorted(sfiles)],
            }
            oracle, titems = oracle_for(j.entries, t["first"], t["last"])
            tb, ob = jbytes(task), jbytes(oracle)
            out[f"{SUITE_REL}/tasks/{t['id']}/task.json"] = tb
            out[f"{SUITE_REL}/tasks/{t['id']}/start.tar"] = start
            out[f"{SUITE_REL}/oracles/{t['id']}/oracle.json"] = ob
            pairs += [[f"tasks/{t['id']}/task.json", sha(tb)], [f"tasks/{t['id']}/start.tar", sha(start)],
                      [f"oracles/{t['id']}/oracle.json", sha(ob)]]
            trace["tasks"].append({"task": t["id"], "job": j.job, "first_seq": t["first"], "last_seq": t["last"],
                                   "workflow_hint": t["workflow_hint"], "items": titems})
            summary.append({"id": t["id"], "tier": t["tier"], "apps": apps, "items": len(oracle["items"]),
                            "forbidden": len(oracle["forbidden"])})
    trace["tasks"].sort(key=lambda x: x["task"])
    out[f"{SUITE_REL}/TRACE.json"] = jbytes(trace)
    pairs.sort(key=lambda p: p[0])
    out[f"{FIX_REL}/expected.json"] = jbytes({
        "suite": SUITE_REL,
        "suite_version": sha(jcs_string_pairs(pairs)),
        "files": pairs,
        "tasks": sorted(summary, key=lambda s: s["id"]),
    })
    return out


# ---------------------------------------------------------------- reference self-tests

def self_test():
    errs = []

    def close(a, b, tol, what):
        if abs(a - b) > tol:
            errs.append(f"self-test {what}: {a!r} vs {b!r}")

    # Exact circle (no rounding): the fit recovers centre and radius.
    pts = [[3.0 + 2.5 * math.cos(t), -1.0 + 2.5 * math.sin(t)] for t in [0.1 + k * 0.7 for k in range(9)]]
    cx, cy, r = ref_geom.circle_fit(pts)
    close(cx, 3.0, 1e-12, "circle cx")
    close(cy, -1.0, 1e-12, "circle cy")
    close(r, 2.5, 1e-12, "circle r")
    # Uniform sampling with harmonic form k >= 2: the LS circle is the base circle.
    pts = [[(5 + 0.01 * math.cos(3 * t)) * math.cos(t), (5 + 0.01 * math.cos(3 * t)) * math.sin(t)]
           for t in [2 * math.pi * k / 24 for k in range(24)]]
    cx, cy, r = ref_geom.circle_fit(pts)
    close(r, 5.0, 2e-5, "circle with form r")
    # Plane: a tilted exact plane has zero flatness; two layers at +/-d give 2d / sqrt(1 + slope^2).
    pts = [[x, y, 0.5 + 0.1 * x - 0.2 * y] for x in range(4) for y in range(3)]
    close(ref_geom.flatness_lsq(pts), 0.0, 1e-12, "exact plane")
    pts = [[x, y, (0.01 if (x + y) % 2 else -0.01)] for x in range(4) for y in range(4)]
    close(ref_geom.flatness_lsq(pts), 0.02, 1e-12, "checkerboard plane")
    # Jacobi: eigen-decomposition reproduces the matrix.
    s = [[4.0, 1.0, 0.5], [1.0, 3.0, 0.2], [0.5, 0.2, 1.0]]
    vals, vecs = ref_geom._jacobi_eigen(s)
    for k in range(3):
        v = [vecs[i][k] for i in range(3)]
        for i in range(3):
            close(sum(s[i][m] * v[m] for m in range(3)), vals[k] * v[i], 1e-12, "jacobi")
    # Slot: two parallel lines 10 apart, tilted.
    a = [[0.3 * y / 10, y] for y in range(5)]
    b = [[10 / math.cos(math.atan(0.03)) + 0.3 * y / 10, y] for y in range(5)]
    close(ref_geom.slot_width(a, b), 10.0, 1e-12, "slot width")
    return errs


def main() -> int:
    errs = self_test()
    files = build()
    if "--check" in sys.argv[1:]:
        for rel, b in sorted(files.items()):
            p = ROOT / rel
            if not p.is_file() or p.read_bytes() != b:
                errs.append(f"stale or missing: {rel} (run make_corpus.py)")
        for base in (SUITE_REL + "/tasks", SUITE_REL + "/oracles", FIX_REL + "/golden"):
            for p in sorted((ROOT / base).rglob("*")):
                rel = p.relative_to(ROOT).as_posix()
                if p.is_file() and rel not in files:
                    errs.append(f"unexpected file: {rel}")
    else:
        for rel, b in files.items():
            p = ROOT / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b)
    for e in errs:
        print("FAIL", e)
    print("eval corpus generation check:", "failed" if errs else "ok")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main())
