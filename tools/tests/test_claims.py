"""Tests of tools/claims.py, spec node ECO.COMM.CLAIM (AC-1..AC-6).

Expected results are hand-labelled from the leaf's rules (see the history table in
fixtures/claims/make_repo.py and the expected-*.json files), not produced by running the tool.
Offline: the fixture repository is built locally with fixed dates; PR data comes from JSON files.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "claims.py"
FIX = Path(__file__).resolve().parent / "fixtures" / "claims"
NOW = "2026-12-01T00:00:00Z"

sys.path.insert(0, str(ROOT / "tools"))

import claims  # noqa: E402


def run(*args):
    r = subprocess.run([sys.executable, str(TOOL), *args], stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    return r.returncode, r.stdout, r.stderr.decode("utf-8")


@unittest.skipIf(shutil.which("git") is None, "git not installed")
class StaleTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp(prefix="claims-")
        work = Path(cls.tmp)
        bundle = work / "repo.bundle"
        subprocess.run([sys.executable, str(FIX / "make_repo.py"), str(work / "src"), "--bundle", str(bundle)],
                       check=True)
        cls.repo = work / "clone"
        subprocess.run(["git", "-c", "core.autocrlf=false", "clone", "-q", str(bundle), str(cls.repo)], check=True)

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, ignore_errors=True)

    def stale(self, prs=FIX / "prs.json", now=NOW):
        return run("stale", "--repo", str(self.repo), "--prs", str(prs), "--now", now, "--json")

    def test_ac1_lists_exactly_node_a(self):
        code, out, err = self.stale()
        self.assertEqual(code, 0, err)
        self.assertEqual(out, (FIX / "expected-stale.json").read_bytes())

    def test_ac6_byte_identical(self):
        self.assertEqual(self.stale()[1], self.stale()[1])

    def test_open_pr_branch_name_counts(self):
        tmp = Path(self.tmp) / "prs-branch.json"
        tmp.write_text(json.dumps([{"number": 1, "title": "x", "body": "", "headRefName": "claim/A", "state": "OPEN"}]))
        out = json.loads(self.stale(prs=tmp)[1])
        self.assertEqual([s["id"] for s in out["stale"]], ["C"])

    def test_later_now_and_empty_pr_list(self):
        tmp = Path(self.tmp) / "prs-empty.json"
        tmp.write_text("[]")
        # 2026-12-01T00:00:01Z: E (claimed 2026-11-17T00:00:00Z) is now more than 14 days old.
        out = json.loads(self.stale(prs=tmp, now="2026-12-01T00:00:01Z")[1])
        self.assertEqual([s["id"] for s in out["stale"]], ["A", "C", "E"])
        self.assertEqual(out["stale"][2], {"id": "E", "claimed_by": "erin", "claimed_at": "2026-11-17T00:00:00Z"})

    def test_bad_json_is_claims_input(self):
        tmp = Path(self.tmp) / "bad.json"
        tmp.write_text("{not json")
        code, _out, err = self.stale(prs=tmp)
        self.assertEqual(code, 1)
        self.assertIn("CLAIMS-INPUT", err)

    def test_not_a_repo_is_claims_git(self):
        with tempfile.TemporaryDirectory() as d:
            if subprocess.run(["git", "-C", d, "rev-parse"], stderr=subprocess.DEVNULL).returncode == 0:
                self.skipTest("temp dir is inside a git work tree")
            code, _out, err = run("stale", "--repo", d, "--prs", str(FIX / "prs.json"), "--now", NOW)
        self.assertEqual(code, 1)
        self.assertIn("CLAIMS-GIT", err)


class TraceTest(unittest.TestCase):
    def trace(self, name):
        return run("trace", "--pr", str(FIX / name), "--tree", str(FIX / "tree"))

    def test_ac2_ok(self):
        code, out, err = self.trace("pr-ok.json")
        self.assertEqual(code, 0, err)
        self.assertEqual(out, (FIX / "expected-trace-ok.json").read_bytes())

    def test_ac3_none(self):
        code, out, err = self.trace("pr-none.json")
        self.assertEqual(code, 1)
        self.assertIn("TRACE-NONE", err)
        self.assertEqual(json.loads(out)["codes"], ["TRACE-NONE"])

    def test_ac4_not_leaf(self):
        code, out, err = self.trace("pr-open-node.json")
        self.assertEqual(code, 1)
        self.assertIn("TRACE-NOT-LEAF ALN.RPS.SEED", err)
        self.assertEqual(json.loads(out)["result"], "fail")

    def test_ac5_no_spec_exempt(self):
        code, out, err = self.trace("pr-nospec.json")
        self.assertEqual(code, 0, err)
        doc = json.loads(out)
        self.assertEqual(doc["exempt"], "no-spec")
        self.assertEqual(doc["result"], "ok")
        self.assertIn(b'"exempt": "no-spec"', out)

    def test_unknown_and_frozen(self):
        code, out, _err = self.trace("pr-unknown.json")
        self.assertEqual(code, 1)
        doc = json.loads(out)
        self.assertEqual(doc["nodes"], ["ALN.RPS", "ALN.RPS.NOPE", "ALN.RPS.OLD"])
        self.assertEqual(doc["codes"], ["TRACE-NOT-LEAF ALN.RPS", "TRACE-UNKNOWN ALN.RPS.NOPE"])


class UnitTest(unittest.TestCase):
    def test_mentions_word_boundary(self):
        self.assertTrue(claims.mentions("C", "spec(C): leaf"))
        self.assertTrue(claims.mentions("A.B", "fixes A.B."))
        self.assertFalse(claims.mentions("A", "ALPHA"))
        self.assertFalse(claims.mentions("A", "A.X"))
        self.assertFalse(claims.mentions("A.B", "A.B.C"))
        self.assertFalse(claims.mentions("B", "X.B"))

    def test_claim_value(self):
        self.assertEqual(claims.claim_value("---\nid: X\nclaimed_by:\n---\nclaimed_by: body\n"), "")
        self.assertEqual(claims.claim_value("---\nid: X\nclaimed_by: ann\n---\n"), "ann")


if __name__ == "__main__":
    unittest.main()
