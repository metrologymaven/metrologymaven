#!/usr/bin/env python3
"""Independent reference for ALN.BFIT.STOP; writes stop-traces.json and stop-manifest.json.

Stdlib only. Shares no code with the Rust crate fid-align-solve: the monitor below is
written from spec/aln/bfit/stop/NODE.md and the rule order pinned in the crate's module
documentation (`stop.rs`), and every trace carries a hand-written label (verdict and
iteration) that the reference must reproduce before anything is written.

Rules (default criteria: max_iterations 100, translation_tol 1e-9 m, rotation_tol 1e-9 rad,
objective_rel_tol 1e-12, divergence_window 5):

  start      f_0 = objective of the start pose (iteration 0); best = (0, f_0).
  update k   iteration k must be last + 1 (stop_bad_iteration), no update after a terminal
             verdict (stop_monitor_finished); NaN in dt, dr, objective (in that order) is
             nan_in_iteration naming the input, then an infinity or a negative increment is
             invalid_iteration_value naming it; -0.0 is read as +0.0.
             rel = |f_k - f_{k-1}| / max(|f_{k-1}|, 1e-300)
             small run += 1 if rel <= objective_rel_tol else 0
             rise run  += 1 if f_k > f_{k-1} else 0
             verdict, first that applies:
               stopped(diverged)        rise run >= divergence_window
               converged(increment)     dt <= translation_tol and dr <= rotation_tol
               converged(objective)     small run >= 2
               stopped(max_iterations)  k == max_iterations
               continue
             best = (k, f_k) if f_k < best objective (a tie keeps the earlier iteration)
  outcome    converged(..): status ok, pose of the last iteration;
             stopped(..):   status not_converged, pose of the best iteration.
             stopped_by (ALN.QA.HIST), always a declared criterion name:
             converged(translation_tol) for the increment rule,
             converged(objective_rel_tol), limit(max_iterations),
             diverged(divergence_window).

Criteria settings: applied over the defaults in input order; an unknown name is
unknown_stop_criterion, a repeated one duplicate_stop_criterion; a tolerance must be finite
and >= 0, max_iterations and divergence_window integers in 1..=100000, else
invalid_stop_criteria naming the field.

Non-finite numbers are written as the strings "NaN", "Infinity", "-Infinity".
Python floats are IEEE 754 binary64 and `-`, `/` are correctly rounded, as in Rust; repr()
round-trips, so the Rust test parses the same bits.

Outputs (CC-BY-4.0, project-authored):
  stop-traces.json    traces, error traces and criteria cases with expected results (AC-1..AC-4)
  stop-manifest.json  a registry manifest with `align.run_bestfit@1` declaring the five
                      criteria with units and defaults (AC-5); checked in CI by
                      `cargo xtask convergence-check`.

Run:  python fixtures/aln/bfit/make_stop_traces.py [--check]
With --check nothing is written; exits non-zero if an output differs.
"""

import json
import math
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

DEFAULTS = {
    "max_iterations": 100,
    "translation_tol": 1e-9,
    "rotation_tol": 1e-9,
    "objective_rel_tol": 1e-12,
    "divergence_window": 5,
}
NAMES = ["max_iterations", "translation_tol", "rotation_tol", "objective_rel_tol", "divergence_window"]
INTEGER = {"max_iterations", "divergence_window"}
LIMIT = 100000
FLOOR = 1e-300


class StopErr(Exception):
    def __init__(self, code, field=None, iteration=None):
        super().__init__(code)
        self.code = code
        self.field = field
        self.iteration = iteration

    def as_json(self):
        out = {"error": self.code}
        if self.field is not None:
            out["field"] = self.field
        if self.iteration is not None:
            out["iteration"] = self.iteration
        return out


def criteria_from(settings):
    c = dict(DEFAULTS)
    seen = set()
    for name, value in settings:
        if name not in NAMES:
            raise StopErr("unknown_stop_criterion")
        if name in seen:
            raise StopErr("duplicate_stop_criterion", name)
        seen.add(name)
        if name in INTEGER:
            ok = math.isfinite(value) and 1 <= value <= LIMIT and value == int(value)
            if not ok:
                raise StopErr("invalid_stop_criteria", name)
            c[name] = int(value)
        else:
            if math.isnan(value) or math.isinf(value) or value < 0:
                raise StopErr("invalid_stop_criteria", name)
            c[name] = value + 0.0 if value != 0 else 0.0
    return c


def check(iteration, field, x, increment):
    if math.isnan(x):
        raise StopErr("nan_in_iteration", field, iteration)
    if x == 0:
        x = 0.0
    if math.isinf(x) or (increment and x < 0):
        raise StopErr("invalid_iteration_value", field, iteration)
    return x


def run(criteria, f0, steps):
    """Feeds steps (iteration 1, 2, ...) until a terminal verdict; returns the result dict."""
    f0 = check(0, "objective", f0, False)
    prev, small, rise, best = f0, 0, 0, (0, f0)
    for k, (dt, dr, f) in enumerate(steps, start=1):
        for field, x in (("dt", dt), ("dr", dr), ("objective", f)):
            if math.isnan(x):
                raise StopErr("nan_in_iteration", field, k)
        dt = check(k, "dt", dt, True)
        dr = check(k, "dr", dr, True)
        f = check(k, "objective", f, False)
        rel = abs(f - prev) / max(abs(prev), FLOOR)
        small = small + 1 if rel <= criteria["objective_rel_tol"] else 0
        rise = rise + 1 if f > prev else 0
        if rise >= criteria["divergence_window"]:
            verdict = "stopped(diverged)"
        elif dt <= criteria["translation_tol"] and dr <= criteria["rotation_tol"]:
            verdict = "converged(increment)"
        elif small >= 2:
            verdict = "converged(objective)"
        elif k >= criteria["max_iterations"]:
            verdict = "stopped(max_iterations)"
        else:
            verdict = "continue"
        prev = f
        if f < best[1]:
            best = (k, f)
        if verdict != "continue":
            converged = verdict.startswith("converged")
            pose = (k, f) if converged else best
            stopped_by = {
                "converged(increment)": "converged(translation_tol)",
                "converged(objective)": "converged(objective_rel_tol)",
                "stopped(max_iterations)": "limit(max_iterations)",
                "stopped(diverged)": "diverged(divergence_window)",
            }[verdict]
            return {
                "verdict": verdict,
                "iteration": k,
                "best_iteration": best[0],
                "best_objective": best[1],
                "status": "ok" if converged else "not_converged",
                "pose_iteration": pose[0],
                "pose_objective": pose[1],
                "stopped_by": stopped_by,
            }
    return {"verdict": "continue", "iteration": len(steps)}


# ------------------------------------------------------------------ hand-written traces
# Each trace: name, criteria settings, f_0, steps [dt, dr, f], and the hand label
# (verdict, iteration) the reference must reproduce.

BIG = 1e-3  # an increment far above the default tolerances


def oscillating(n):
    """Objective alternating 2.0, 1.0 with large steps: no 5 rises in a row, no small change."""
    return [[BIG, BIG, 2.0 if k % 2 == 1 else 1.0] for k in range(1, n + 1)]


def slow_plateau(n):
    """Objective falling by a relative 1e-6 per iteration with large steps: never converges."""
    out, f = [], 1.0
    for _ in range(n):
        f = f - f * 1e-6
        out.append([BIG, BIG, f])
    return out


TRACES = [
    # converging by increment (AC-1)
    ("increment_converges", [], 10.0,
     [[1e-3, 1e-3, 5.0], [1e-5, 1e-6, 2.0], [1e-9, 1.0000000000000002e-9, 1.5], [5e-10, 1e-9, 1.4999]],
     ("converged(increment)", 4)),
    ("increment_at_boundary_iteration_one", [], 1.0, [[1e-9, 1e-9, 0.5]], ("converged(increment)", 1)),
    ("increment_negative_zero", [], 1.0, [[BIG, BIG, 0.9], [-0.0, 0.0, 0.8]], ("converged(increment)", 2)),
    ("increment_needs_both", [], 1.0,
     [[0.0, BIG, 0.9], [BIG, 0.0, 0.8], [0.0, 2e-9, 0.7], [1e-10, 1e-10, 0.6]], ("converged(increment)", 4)),
    # converging by objective (AC-1)
    ("objective_converges", [], 1.0,
     [[BIG, BIG, 0.5], [BIG, BIG, 0.25], [BIG, BIG, 0.25 - 1e-14], [BIG, BIG, 0.25 - 2e-14]],
     ("converged(objective)", 4)),
    ("objective_needs_two_consecutive", [], 1.0,
     [[BIG, BIG, 1.0], [BIG, BIG, 0.5], [BIG, BIG, 0.5], [BIG, BIG, 0.4], [BIG, BIG, 0.4], [BIG, BIG, 0.4]],
     ("converged(objective)", 6)),
    ("objective_plateau_zero", [], 0.0, [[BIG, BIG, 0.0], [BIG, BIG, 0.0]], ("converged(objective)", 2)),
    # subnormal objectives: the 1e-300 floor makes halving a negligible relative change
    ("objective_tiny_values_floor", [], 4e-320, [[BIG, BIG, 2e-320], [BIG, BIG, 1e-320]],
     ("converged(objective)", 2)),
    ("objective_negative_values", [], -1.0,
     [[BIG, BIG, -2.0], [BIG, BIG, -2.0 - 1e-12], [BIG, BIG, -2.0 - 2e-12]], ("converged(objective)", 3)),
    # diverging (AC-2)
    ("diverges_from_start", [], 1.0,
     [[BIG, BIG, 1.1], [BIG, BIG, 1.2], [BIG, BIG, 1.3], [BIG, BIG, 1.4], [BIG, BIG, 1.5], [BIG, BIG, 1.6]],
     ("stopped(diverged)", 5)),
    ("diverges_after_improving", [], 10.0,
     [[BIG, BIG, 8.0], [BIG, BIG, 6.0], [BIG, BIG, 5.0], [BIG, BIG, 5.5], [BIG, BIG, 6.0], [BIG, BIG, 7.0],
      [BIG, BIG, 8.0], [BIG, BIG, 9.0]],
     ("stopped(diverged)", 8)),
    ("diverged_beats_small_step", [], 1.0,
     [[BIG, BIG, 2.0], [BIG, BIG, 3.0], [BIG, BIG, 4.0], [BIG, BIG, 5.0], [0.0, 0.0, 6.0]],
     ("stopped(diverged)", 5)),
    ("rise_run_interrupted", [], 1.0,
     [[BIG, BIG, 2.0], [BIG, BIG, 3.0], [BIG, BIG, 4.0], [BIG, BIG, 5.0], [BIG, BIG, 5.0], [BIG, BIG, 6.0],
      [BIG, BIG, 7.0], [BIG, BIG, 8.0], [BIG, BIG, 9.0], [1e-10, 1e-10, 8.5]],
     ("converged(increment)", 10)),
    ("overflowing_change", [], -1.7e308,
     [[BIG, BIG, 1.7e308], [BIG, BIG, 1.0], [0.0, 0.0, 0.5]], ("converged(increment)", 3)),
    # plateauing and oscillating: never converge (AC-3)
    ("oscillating_never_converges", [], 3.0, oscillating(105), ("stopped(max_iterations)", 100)),
    ("slow_plateau_never_converges", [], 1.0, slow_plateau(100), ("stopped(max_iterations)", 100)),
    # non-default criteria
    ("custom_limit_converged_on_last", [("max_iterations", 3.0)], 1.0,
     [[BIG, BIG, 0.9], [BIG, BIG, 0.8], [0.0, 0.0, 0.7]], ("converged(increment)", 3)),
    ("custom_limit_stops", [("max_iterations", 3.0)], 1.0,
     [[BIG, BIG, 0.9], [BIG, BIG, 0.95], [BIG, BIG, 0.7], [BIG, BIG, 0.6]], ("stopped(max_iterations)", 3)),
    ("custom_window_two", [("divergence_window", 2.0)], 1.0,
     [[BIG, BIG, 0.5], [BIG, BIG, 0.6], [BIG, BIG, 0.7]], ("stopped(diverged)", 3)),
    ("custom_tolerances", [("translation_tol", 1e-6), ("rotation_tol", 1e-5), ("objective_rel_tol", 0.0)], 1.0,
     [[1e-6, 2e-5, 0.9], [1e-6, 1e-5, 0.8]], ("converged(increment)", 2)),
    ("custom_zero_rel_tol_exact_repeat", [("objective_rel_tol", 0.0)], 1.0,
     [[BIG, BIG, 0.5], [BIG, BIG, 0.5 - 1e-16], [BIG, BIG, 0.5 - 1e-16], [BIG, BIG, 0.5 - 1e-16]],
     ("converged(objective)", 4)),
]

ERROR_TRACES = [
    ("nan_objective", 1.0, [[BIG, BIG, 0.9], [BIG, BIG, 0.8], [BIG, BIG, math.nan]],
     ("nan_in_iteration", "objective", 3)),
    ("nan_dt_before_objective", 1.0, [[math.nan, BIG, math.nan]], ("nan_in_iteration", "dt", 1)),
    ("nan_dr", 1.0, [[BIG, math.nan, 0.5]], ("nan_in_iteration", "dr", 1)),
    ("nan_after_infinity", 1.0, [[math.inf, BIG, math.nan]], ("nan_in_iteration", "objective", 1)),
    ("infinite_objective", 1.0, [[BIG, BIG, math.inf]], ("invalid_iteration_value", "objective", 1)),
    ("negative_dt", 1.0, [[BIG, BIG, 0.9], [-1e-12, BIG, 0.8]], ("invalid_iteration_value", "dt", 2)),
    ("infinite_dr", 1.0, [[BIG, -math.inf, 0.8]], ("invalid_iteration_value", "dr", 1)),
    ("nan_start", math.nan, [[BIG, BIG, 0.5]], ("nan_in_iteration", "objective", 0)),
    ("infinite_start", -math.inf, [[BIG, BIG, 0.5]], ("invalid_iteration_value", "objective", 0)),
]

CRITERIA_CASES = [
    ("translation_tol_negative", [("translation_tol", -1.0)], ("invalid_stop_criteria", "translation_tol")),
    ("rotation_tol_nan", [("rotation_tol", math.nan)], ("invalid_stop_criteria", "rotation_tol")),
    ("objective_rel_tol_tiny_negative", [("objective_rel_tol", -1e-300)], ("invalid_stop_criteria", "objective_rel_tol")),
    ("translation_tol_infinite", [("translation_tol", math.inf)], ("invalid_stop_criteria", "translation_tol")),
    ("max_iterations_zero", [("max_iterations", 0.0)], ("invalid_stop_criteria", "max_iterations")),
    ("max_iterations_over_limit", [("max_iterations", 100001.0)], ("invalid_stop_criteria", "max_iterations")),
    ("max_iterations_negative", [("max_iterations", -1.0)], ("invalid_stop_criteria", "max_iterations")),
    ("max_iterations_fraction", [("max_iterations", 2.5)], ("invalid_stop_criteria", "max_iterations")),
    ("divergence_window_zero", [("divergence_window", 0.0)], ("invalid_stop_criteria", "divergence_window")),
    ("divergence_window_negative", [("divergence_window", -5.0)], ("invalid_stop_criteria", "divergence_window")),
    ("first_bad_in_input_order", [("rotation_tol", -1.0), ("translation_tol", -1.0)],
     ("invalid_stop_criteria", "rotation_tol")),
    ("unknown_name", [("max_iter", 10.0)], ("unknown_stop_criterion", None)),
    ("duplicate_name", [("translation_tol", 1e-6), ("translation_tol", 1e-6)],
     ("duplicate_stop_criterion", "translation_tol")),
    ("all_set", [("max_iterations", 100000.0), ("translation_tol", 1e-6), ("rotation_tol", -0.0),
                 ("objective_rel_tol", 1e-10), ("divergence_window", 1.0)], None),
    ("none_set", [], None),
]


def enc(x):
    if isinstance(x, float):
        if math.isnan(x):
            return "NaN"
        if math.isinf(x):
            return "Infinity" if x > 0 else "-Infinity"
    return x


def build_traces():
    out = {
        "about": "ALN.BFIT.STOP stopping traces. Generated by make_stop_traces.py (independent "
                 "reference); every trace carries a hand-written label the reference reproduces. "
                 "CC-BY-4.0.",
        "defaults": DEFAULTS,
        "traces": [],
        "error_traces": [],
        "criteria_cases": [],
    }
    for name, settings, f0, steps, label in TRACES:
        c = criteria_from(settings)
        res = run(c, f0, steps)
        got = (res["verdict"], res["iteration"])
        if got != label:
            sys.exit(f"{name}: reference gives {got}, hand label {label}")
        out["traces"].append({
            "name": name,
            "settings": [[n, enc(v)] for n, v in settings],
            "start_objective": enc(f0),
            "steps": [[enc(a), enc(b), enc(f)] for a, b, f in steps],
            "expect": res,
        })
    for name, f0, steps, label in ERROR_TRACES:
        try:
            run(dict(DEFAULTS), f0, steps)
        except StopErr as e:
            got = (e.code, e.field, e.iteration)
            if got != label:
                sys.exit(f"{name}: reference gives {got}, hand label {label}")
            out["error_traces"].append({
                "name": name,
                "start_objective": enc(f0),
                "steps": [[enc(a), enc(b), enc(f)] for a, b, f in steps],
                "expect": e.as_json(),
            })
        else:
            sys.exit(f"{name}: reference accepted the trace")
    for name, settings, label in CRITERIA_CASES:
        entry = {"name": name, "settings": [[n, enc(v)] for n, v in settings]}
        try:
            c = criteria_from(settings)
        except StopErr as e:
            if (e.code, e.field) != label:
                sys.exit(f"{name}: reference gives {(e.code, e.field)}, hand label {label}")
            entry["expect"] = e.as_json()
        else:
            if label is not None:
                sys.exit(f"{name}: reference accepted, hand label {label}")
            entry["expect"] = {"criteria": c}
        out["criteria_cases"].append(entry)
    return out


MEANINGS = {
    "max_iterations": "Stop with stopped(max_iterations) and status not_converged when this many iterations "
                      "ran without convergence.",
    "translation_tol": "Converged by increment when the translation step is at or below this length and the "
                       "rotation step within rotation_tol.",
    "rotation_tol": "Converged by increment when the rotation step is at or below this angle and the "
                    "translation step within translation_tol.",
    "objective_rel_tol": "Converged by objective when the relative objective change is at or below this value "
                         "on two consecutive iterations.",
    "divergence_window": "Stop with stopped(diverged) and status not_converged when the objective rose on this "
                         "many consecutive iterations.",
}
UNITS = {"max_iterations": "1", "translation_tol": "m", "rotation_tol": "rad", "objective_rel_tol": "1",
         "divergence_window": "1"}
DEFAULT_TEXT = {"max_iterations": "100", "translation_tol": "1e-9", "rotation_tol": "1e-9",
                "objective_rel_tol": "1e-12", "divergence_window": "5"}


MAX_MEANING_CHARS = 120  # fid-xtask convergence-check limit


def build_manifest():
    for n in NAMES:
        if len(MEANINGS[n]) > MAX_MEANING_CHARS:
            sys.exit(f"meaning of {n} exceeds {MAX_MEANING_CHARS} characters")
    return {
        "format": 1,
        "quantity_catalogue_digest": "sha256:" + "0" * 64,
        "commands": [{
            "id": "align.run_bestfit",
            "major": 1,
            "iterative": True,
            "convergence": [
                {"name": n, "unit": UNITS[n], "default": DEFAULT_TEXT[n], "meaning": MEANINGS[n]} for n in NAMES
            ],
        }],
    }


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def main():
    check_only = "--check" in sys.argv[1:]
    outputs = {"stop-traces.json": dump(build_traces()), "stop-manifest.json": dump(build_manifest())}
    bad = 0
    for name, text in outputs.items():
        path = HERE / name
        if check_only:
            have = path.read_bytes().decode("utf-8") if path.exists() else None
            if have != text:
                print(f"{name}: differs from the reference output", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(text.encode("utf-8"))
    if bad:
        sys.exit(1)
    print("make_stop_traces: ok" if check_only else "make_stop_traces: written")


if __name__ == "__main__":
    main()
