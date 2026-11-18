"""GEO.NURBS.REF fixtures: coverage (AC-3) and a fast partial regeneration (part of AC-1).

The full byte-identical regeneration takes minutes (`python tools/fixtures/nurbs_oracle.py
--out fixtures/geo/nurbs --seed 40 --check`); this test rebuilds the seeded geometry of
ref_set_v1 and re-evaluates a sample of reference rows exactly, which must reproduce the committed
bytes of those values.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import json
import random
import subprocess
import sys
import unittest
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIX = ROOT / "fixtures" / "geo" / "nurbs"
sys.path.insert(0, str(ROOT / "tools" / "fixtures"))

import nurbs_oracle as oracle  # noqa: E402


class NurbsRefTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ref = json.loads((FIX / "ref_set_v1.json").read_text(encoding="utf-8"))

    def test_coverage_script_passes(self):
        r = subprocess.run([sys.executable, str(ROOT / "tools" / "fixtures" / "nurbs_coverage.py"), str(FIX)],
                           capture_output=True, text=True)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("coverage: OK", r.stdout)

    def test_seeded_geometry_reproduces(self):
        curves, surfaces = oracle.build_ref_entities(random.Random(self.ref["seed"]))
        built = {e.id: e for e in curves + surfaces}
        for ent in self.ref["entities"]:
            geo = {k: v for k, v in ent.items() if k not in ("eval", "domain_checks", "invert")}
            self.assertEqual(json.loads(json.dumps(built[ent["id"]].json_geometry())), geo, ent["id"])

    def test_sample_rows_reproduce_exactly(self):
        curves, surfaces = oracle.build_ref_entities(random.Random(self.ref["seed"]))
        built = {e.id: e for e in curves + surfaces}
        for ident in ("circle", "c01n", "c03r", "c25r", "s0303n", "sphere"):
            ent = next(e for e in self.ref["entities"] if e["id"] == ident)
            e = built[ident]
            for row in ent["eval"][:6]:
                if e.surface:
                    u, v = (oracle.struct.unpack(">d", int(row[k]["hex"], 16).to_bytes(8, "big"))[0] for k in "uv")
                    got = oracle.eval_surface_record(e, u, v, row.get("tag"))[0]
                else:
                    t = oracle.struct.unpack(">d", int(row["t"]["hex"], 16).to_bytes(8, "big"))[0]
                    got = oracle.eval_curve_record(e, t, row.get("tag"))[0]
                self.assertEqual(json.loads(json.dumps(got)), row, ident)

    def test_circle_radius(self):
        circle = next(e for e in self.ref["entities"] if e["id"] == "circle")
        self.assertGreaterEqual(len(circle["eval"]), 100)
        for row in circle["eval"]:
            x, y, _ = (Fraction(v) for v in row["p"]["dec"])
            self.assertLessEqual(abs(x * x + y * y - 1), Fraction(2, 10 ** 15))


if __name__ == "__main__":
    unittest.main()
