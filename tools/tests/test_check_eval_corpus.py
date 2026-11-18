"""Acceptance tests of spec node AGENT.EVAL.CORPUS (AC-2, AC-3, AC-4 and REQ-004) for the corpus
under suite/, checked by tools/check_eval_corpus.py. AC-1 (`fid-eval suite check`) is the Rust
test crates/fid-eval-suite/tests/corpus.rs.

Each error-path case copies the corpus, its golden journals, datasets.toml and the catalog into a
temporary folder and makes one change. The expected findings are hand-labelled from the leaf's
rules; they were not produced by running the checker.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import json
import shutil
import struct
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import check_eval_corpus as cec  # noqa: E402

GOLDEN = "fixtures/agent/eval/corpus/golden"


class CorpusCopy:
    def __init__(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        shutil.copytree(ROOT / "suite", self.root / "suite", ignore=shutil.ignore_patterns("redteam"))
        shutil.copytree(ROOT / GOLDEN, self.root / GOLDEN)
        shutil.copy(ROOT / "datasets.toml", self.root / "datasets.toml")
        (self.root / "agent").mkdir()
        shutil.copy(ROOT / "agent" / "catalog.json", self.root / "agent" / "catalog.json")

    def path(self, rel):
        return self.root / rel

    def edit(self, rel, fn):
        p = self.path(rel)
        doc = json.loads(p.read_text(encoding="utf-8"))
        doc = fn(doc) or doc
        p.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")

    def run(self):
        return cec.check(self.root / "suite", self.root)

    def close(self):
        self.tmp.cleanup()


class CorpusTests(unittest.TestCase):
    def setUp(self):
        self.c = CorpusCopy()

    def tearDown(self):
        self.c.close()

    def assertFinding(self, errs, *parts):
        self.assertTrue(any(all(p in e for p in parts) for e in errs), f"no finding with {parts}: {errs}")

    def test_shipped_corpus_passes(self):
        self.assertEqual(cec.check(ROOT / "suite"), [])

    def test_copy_passes(self):
        self.assertEqual(self.c.run(), [])

    def test_ac3_proprietary_licence_names_file(self):
        def f(t):
            for lic in t["licences"]:
                if lic["file"] == "in/drawing.json":
                    lic["spdx"] = "proprietary"
        self.c.edit("suite/tasks/inspect-bore-b1/task.json", f)
        errs = self.c.run()
        self.assertFinding(errs, "inspect-bore-b1", "in/drawing.json", "'proprietary'")

    def test_allowlisted_spdx_passes(self):
        def f(t):
            t["licences"][0]["spdx"] = "CC-BY-4.0"
        self.c.edit("suite/tasks/inspect-bore-b1/task.json", f)
        self.assertEqual(self.c.run(), [])

    def test_unlicensed_archive_file(self):
        def f(t):
            t["licences"] = [x for x in t["licences"] if x["file"] != "README.txt"]
        self.c.edit("suite/tasks/probe-keyway-width/task.json", f)
        self.assertFinding(self.c.run(), "probe-keyway-width", "README.txt", "no licence entry")

    def test_ac2_value_off_by_one_bit(self):
        def f(o):
            a = o["items"][0]["value"]["amount"]
            n = struct.unpack(">Q", bytes.fromhex(a[2:]))[0] + 1
            o["items"][0]["value"]["amount"] = "0x" + struct.pack(">Q", n).hex()
        self.c.edit("suite/oracles/inspect-bore-b1/oracle.json", f)
        self.assertFinding(self.c.run(), "inspect-bore-b1", "B1-DIA", "value")

    def test_ac2_outcome_changed(self):
        def f(o):
            for it in o["items"]:
                if it["characteristic"] == "H2-DIA":
                    it["outcome"] = "CONFORMS"
        self.c.edit("suite/oracles/inspect-plate-holes/oracle.json", f)
        self.assertFinding(self.c.run(), "inspect-plate-holes", "H2-DIA", "outcome")

    def test_ac2_decimal_amount_with_same_bits_passes(self):
        def f(o):
            a = o["items"][0]["value"]["amount"]
            o["items"][0]["value"]["amount"] = struct.unpack(">d", bytes.fromhex(a[2:]))[0]
        self.c.edit("suite/oracles/inspect-bore-b1/oracle.json", f)
        self.assertEqual(self.c.run(), [])

    def test_missing_oracle_item(self):
        def f(o):
            o["items"] = [it for it in o["items"] if it["characteristic"] != "PCD"]
        self.c.edit("suite/oracles/probe-bolt-circle/oracle.json", f)
        self.assertFinding(self.c.run(), "probe-bolt-circle", "PCD", "missing from the oracle")

    def test_golden_entry_tampered(self):
        def f(g):
            g["entries"][1]["note"] = "edited"
        self.c.edit(f"{GOLDEN}/JOB.ACC.01.json", f)
        errs = self.c.run()
        self.assertFinding(errs, "JOB.ACC.01", "entry 2 hash")
        self.assertFinding(errs, "JOB.ACC.01", "digest")

    def test_transcribed_result_must_stay_label_only(self):
        def f(g):
            r = g["entries"][0]["result_records"][0]
            r["decision"].update(rule="dr:simple-acceptance@1", outcome="CONFORMS")
        self.c.edit(f"{GOLDEN}/JOB.ACC.03.json", f)
        self.assertFinding(self.c.run(), "report-import-cmm", "transcribed C1", "as-reported-by-source")

    def test_req004_formula_flag_required(self):
        def f(tr):
            for row in tr["tasks"]:
                for it in row["items"]:
                    if it["characteristic"] == "T1":
                        it["formula"] = False
        self.c.edit("suite/TRACE.json", f)
        self.assertFinding(self.c.run(), "T1", "formula flag")

    def test_ac4_missing_tier(self):
        shutil.rmtree(self.c.path("suite/tasks/probe-flange-reeval"))
        shutil.rmtree(self.c.path("suite/oracles/probe-flange-reeval"))
        self.c.edit("suite/TRACE.json",
                    lambda tr: tr.update(tasks=[r for r in tr["tasks"] if r["task"] != "probe-flange-reeval"]))
        self.assertFinding(self.c.run(), "app probe", "full_job")

    def test_req001_unexercised_catalog_workflow(self):
        self.c.edit("agent/catalog.json",
                    lambda c: c.update(workflows=[{"id": "inspect.measure_bore", "app": "inspect"}]))
        self.assertFinding(self.c.run(), "inspect.measure_bore", "exercised by no task")

    def test_catalog_workflow_of_app_without_golden_is_ignored(self):
        self.c.edit("agent/catalog.json",
                    lambda c: c.update(workflows=[{"id": "network.dispatch", "app": "network"}]))
        self.assertEqual(self.c.run(), [])


if __name__ == "__main__":
    unittest.main()
