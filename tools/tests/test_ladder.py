"""Tests of tools/ladder.py, the maintainer-eligibility report of spec node ECO.COMM.LADDER.

The history fixture tools/tests/fixtures/ladder/git-log.txt is written by make_fixtures.py next
to it; the review exports are hand-written. Every expected value below was labelled by hand from
the spec's rules (see make_fixtures.py's docstring), never produced by running the tool. All
names are fictional.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import contextlib
import importlib.util
import io
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import ladder  # noqa: E402

FIX = ROOT / "tools" / "tests" / "fixtures" / "ladder"
LOG = str(FIX / "git-log.txt")
REVIEWS = str(FIX / "reviews.json")
EMPTY_BODY = str(FIX / "reviews-empty-body.json")
NOW = "2026-12-01T00:00:00Z"
REVERT_AT = "2026-11-21T00:00:00Z"  # dave's revert of carol's KRN.SOLVE split
AFTER_70_DAYS = "2027-01-30T00:00:00Z"


def run(*argv, log=LOG, reviews=REVIEWS, now=NOW, config=None):
    args = ["--log", log, "--reviews", reviews, "--now", now] + list(argv)
    if config is not None:
        args += ["--config", config]
    out, err = io.BytesIO(), io.StringIO()
    with contextlib.redirect_stderr(err):
        code = ladder.main(args, out=out)
    return code, out.getvalue(), err.getvalue()


def report(*argv, **kw):
    code, out, err = run("--json", *argv, **kw)
    if code != 0:
        raise AssertionError(f"exit {code}: {err}")
    return json.loads(out.decode("utf-8"))


def names(rep):
    return {d: [e["person"] for e in people] for d, people in rep["eligible"].items()}


class LadderCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="ladder-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def write(self, name, data):
        p = self.tmp / name
        p.write_bytes(data if isinstance(data, bytes) else data.encode("utf-8"))
        return str(p)

    def config(self, **overrides):
        cfg = {"min_expansions": 5, "alt_min_expansions": 3, "alt_min_reviews": 5, "revert_window_days": 60}
        cfg.update(overrides)
        return self.write("config.json", json.dumps(cfg))


class TestAcceptance(LadderCase):
    def test_ac1_eligible_sets(self):
        rep = report()
        self.assertEqual(names(rep), {"GDT": ["alice"], "ALN": ["bob"]})
        self.assertNotIn("carol", json.dumps(rep["eligible"]))

    def test_ac1_counts_and_references(self):
        rep = report()
        alice = rep["eligible"]["GDT"][0]
        self.assertEqual((alice["expansions"], alice["substantive_reviews"]), (5, 0))
        self.assertEqual([c["node"] for c in alice["commits"]],
                         ["GDT.TOL", "GDT.TOL.FORM", "GDT.DAT", "GDT.DAT.REF", "GDT.SYM"])
        self.assertEqual([c["kind"] for c in alice["commits"]], ["split", "leaf", "split", "blocked", "leaf"])
        # +02:00 commit date normalised to UTC
        self.assertEqual(alice["commits"][3]["at"], "2026-09-22T10:00:00Z")
        bob = rep["eligible"]["ALN"][0]
        self.assertEqual((bob["expansions"], bob["substantive_reviews"]), (3, 5))
        # PR 201 reviewed twice counts once, at its first substantive review; PR 206 (commented)
        # is not substantive; PR 203 has an empty body but inline comments; PR 205 is at +02:00.
        self.assertEqual(bob["reviews"], [
            {"pr": 201, "at": "2026-10-11T10:00:00Z"},
            {"pr": 202, "at": "2026-10-13T10:00:00Z"},
            {"pr": 203, "at": "2026-10-14T10:00:00Z"},
            {"pr": 204, "at": "2026-10-15T10:00:00Z"},
            {"pr": 205, "at": "2026-10-16T08:00:00Z"},
        ])
        for e in (alice, bob):
            for c in e["commits"]:
                self.assertRegex(c["sha"], r"^[0-9a-f]{40}$")

    def test_ac1_report_metadata_and_warnings(self):
        rep = report()
        self.assertEqual(rep["now"], NOW)
        self.assertEqual(rep["generated_by"], "tools/ladder.py")
        self.assertIn("Do not edit", rep["notice"])
        self.assertIn("not a nomination", rep["notice"])
        self.assertEqual(rep["thresholds"], {"min_expansions": 5, "alt_min_expansions": 3,
                                             "alt_min_reviews": 5, "revert_window_days": 60})
        self.assertEqual(len(rep["warnings"]), 2)
        self.assertIn("'.GDT' is malformed", rep["warnings"][0])
        self.assertIn("of ALN.GONE matches no split", rep["warnings"][1])

    def test_ac2_carol_eligible_70_days_after_revert(self):
        rep = report(now=AFTER_70_DAYS)
        self.assertEqual(names(rep), {"GDT": ["alice"], "ALN": ["bob"], "KRN": ["carol"]})
        carol = rep["eligible"]["KRN"][0]
        self.assertEqual(carol["expansions"], 6)
        # carol's review of PR 208 (2026-12-05) is after the first --now but before this one
        self.assertEqual(carol["reviews"], [{"pr": 208, "at": "2026-12-05T10:00:00Z"}])

    def test_revert_window_boundary(self):
        # the 60-day window includes its first instant
        self.assertNotIn("KRN", report(now="2027-01-20T00:00:00Z")["eligible"])
        self.assertIn("KRN", report(now="2027-01-20T00:00:01Z")["eligible"])
        # the window comes from config
        cfg = self.config(revert_window_days=5)
        self.assertEqual(names(report(config=cfg))["KRN"], ["carol"])

    def test_ac3_empty_body_reviews_not_substantive(self):
        rep = report(reviews=EMPTY_BODY)
        self.assertEqual(names(rep), {"GDT": ["alice"]})

    def test_ac4_byte_identical(self):
        for fmt in ("--json", "--md"):
            a, b = run(fmt), run(fmt)
            self.assertEqual(a[0], 0)
            self.assertEqual(a, b)

    def test_markdown_report(self):
        code, out, err = run("--md")
        self.assertEqual((code, err), (0, ""))
        text = out.decode("utf-8")
        self.assertEqual(run()[1], out)  # Markdown is the default
        self.assertTrue(text.startswith("<!-- Generated by tools/ladder.py. Do not edit. -->\n"))
        self.assertIn("\nGenerated by tools/ladder.py. Do not edit.\n", text)
        self.assertIn("not a nomination", text)
        self.assertLess(text.index("## ALN"), text.index("## GDT"))
        self.assertIn("- **alice**: 5 expansions, 0 substantive reviews", text)
        self.assertIn("  - review on PR #205 (2026-10-16T08:00:00Z)", text)
        self.assertNotIn("carol", text)
        self.assertNotIn("\r", text)


class TestConfig(LadderCase):
    def test_default_config_is_valid_and_matches_governance(self):
        cfg = ladder.load_config(str(ladder.DEFAULT_CONFIG))
        self.assertEqual((cfg["min_expansions"], cfg["alt_min_expansions"], cfg["alt_min_reviews"],
                          cfg["revert_window_days"]), (5, 3, 5, 60))
        self.assertEqual(cfg["substantive_states"], ["approved", "changes requested"])

    def test_thresholds_from_config(self):
        self.assertEqual(names(report(config=self.config(min_expansions=6))), {"ALN": ["bob"]})
        self.assertEqual(names(report(config=self.config(alt_min_reviews=6))), {"GDT": ["alice"]})
        self.assertEqual(names(report(config=self.config(alt_min_expansions=1, alt_min_reviews=1))),
                         {"GDT": ["alice", "bob"], "ALN": ["bob"]})

    def test_substantive_rule_override(self):
        cfg = self.config(substantive_requires_content=False)
        self.assertEqual(names(report(reviews=EMPTY_BODY, config=cfg)), {"GDT": ["alice"], "ALN": ["bob"]})
        cfg = self.config(substantive_states=["approved"])  # drops PRs 202 and 203
        self.assertEqual(names(report(config=cfg)), {"GDT": ["alice"]})

    def test_aliases_join_identities(self):
        self.assertNotIn("IO", report()["eligible"])
        rep = report(config=str(FIX / "config-aliases.json"))
        self.assertEqual(names(rep)["IO"], ["erin-dev"])
        self.assertEqual(rep["eligible"]["IO"][0]["expansions"], 5)
        # an alias maps a review login too
        cfg = self.config(aliases={"bob": "robert", "1002+bob@users.noreply.github.com": "robert"})
        self.assertEqual(names(report(config=cfg))["ALN"], ["robert"])

    def test_bad_configs(self):
        cases = {
            "notobj.json": "[]",
            "unknown.json": json.dumps({"min_expansions": 5, "alt_min_expansions": 3, "alt_min_reviews": 5,
                                        "revert_window_days": 60, "extra": 1}),
            "missing.json": json.dumps({"min_expansions": 5}),
            "zero.json": json.dumps({"min_expansions": 0, "alt_min_expansions": 3, "alt_min_reviews": 5,
                                     "revert_window_days": 60}),
            "bool.json": json.dumps({"min_expansions": True, "alt_min_expansions": 3, "alt_min_reviews": 5,
                                     "revert_window_days": 60}),
            "states.json": json.dumps({"min_expansions": 5, "alt_min_expansions": 3, "alt_min_reviews": 5,
                                       "revert_window_days": 60, "substantive_states": []}),
            "alias.json": json.dumps({"min_expansions": 5, "alt_min_expansions": 3, "alt_min_reviews": 5,
                                      "revert_window_days": 60, "aliases": {"x": 3}}),
            "syntax.json": "{",
            "deep.json": "[" * 100000 + "]" * 100000,
        }
        for name, text in cases.items():
            with self.subTest(name):
                code, out, err = run(config=self.write(name, text))
                self.assertEqual((code, out), (1, b""))
                self.assertTrue(err.startswith("LADDER-CONFIG: "), err)
                self.assertNotIn("Traceback", err)
        code, _, err = run(config=str(self.tmp / "absent.json"))
        self.assertEqual(code, 1)
        self.assertIn("LADDER-CONFIG", err)


class TestHistory(LadderCase):
    SHA = "a" * 40

    def line(self, sha=None, email="1+x@users.noreply.github.com", when="2026-10-01T00:00:00Z",
             subject="spec(GDT.A): leaf"):
        return f"{sha or self.SHA}\x00X\x00{email}\x00{when}\x00{subject}\n"

    def test_future_commits_and_reverts_ignored(self):
        log = "".join(self.line(f"{i:040x}", subject=f"spec(GDT.N{i}): leaf") for i in range(4))
        log += self.line("f" * 40, when="2026-12-02T00:00:00Z", subject="spec(GDT.LATE): leaf")
        rep = report(log=self.write("log.txt", log), reviews=self.write("r.json", "[]"))
        self.assertEqual(rep["eligible"], {})
        rep = report(log=self.write("log.txt", log), reviews=self.write("r.json", "[]"), now="2026-12-03T00:00:00Z")
        self.assertEqual(rep["eligible"]["GDT"][0]["expansions"], 5)

    def test_revert_attributed_to_split_author_not_reverter(self):
        log = "".join(self.line(f"{i:040x}", email="1+ann@users.noreply.github.com",
                                subject=f"spec(KRN.N{i}): split into 2 children") for i in range(5))
        log += self.line("b" * 40, email="2+ben@users.noreply.github.com", when="2026-11-01T00:00:00Z",
                         subject='Revert "spec(KRN.N2): split into 2 children"')
        rep = report(log=self.write("log.txt", log), reviews=self.write("r.json", "[]"))
        self.assertEqual(rep["eligible"], {})
        self.assertEqual(rep["warnings"], [])

    def test_crlf_log_and_case_insensitive_email(self):
        log = "".join(self.line(f"{i:040x}", email=("ANN@Example.org" if i % 2 else "ann@example.org"),
                                subject=f"spec(ALN.N{i}): blocked on D-00{i}").replace("\n", "\r\n")
                      for i in range(5))
        rep = report(log=self.write("log.txt", log), reviews=self.write("r.json", "[]"))
        self.assertEqual(names(rep), {"ALN": ["ann@example.org"]})

    def test_malformed_log(self):
        cases = {
            "fields": "a" * 40 + "\x00X\x00e@x\x00spec(GDT.A): leaf\n",
            "sha": self.line(sha="xyz"),
            "dup": self.line() + self.line(),
            "date": self.line(when="yesterday"),
            "naive-overflow": self.line(when="0001-01-01T00:00:00+05:00"),
            "email": self.line(email=" "),
        }
        for name, text in cases.items():
            with self.subTest(name):
                code, out, err = run(log=self.write("log.txt", text))
                self.assertEqual((code, out), (1, b""))
                self.assertTrue(err.startswith("LADDER-GIT: "), err)

    def test_bad_utf8_and_size_and_missing(self):
        code, _, err = run(log=self.write("log.txt", b"\xff\xfe" + self.line().encode()))
        self.assertEqual(code, 1)
        self.assertIn("LADDER-GIT", err)
        self.assertIn("UTF-8", err)
        with mock.patch.object(ladder, "MAX_LOG_BYTES", 10):
            code, _, err = run()
        self.assertEqual(code, 1)
        self.assertIn("limit", err)
        code, _, err = run(log=str(self.tmp / "absent.txt"))
        self.assertEqual(code, 1)
        self.assertIn("LADDER-GIT", err)


class TestReviews(LadderCase):
    def review(self, **kw):
        r = {"pr": 1, "reviewer": "bob", "state": "approved", "body": "ok", "inline_comments": 0,
             "at": "2026-10-01T00:00:00Z", "node_ids": ["ALN.A"]}
        r.update(kw)
        return r

    def test_malformed_reviews(self):
        cases = {
            "notlist": {"pr": 1},
            "notobj": [3],
            "pr": [self.review(pr="1")],
            "pr-bool": [self.review(pr=True)],
            "pr-zero": [self.review(pr=0)],
            "reviewer": [self.review(reviewer="")],
            "state": [self.review(state=None)],
            "body": [self.review(body=3)],
            "inline": [self.review(inline_comments=-1)],
            "nodes": [self.review(node_ids="ALN.A")],
            "at": [self.review(at="soon")],
            "at-missing": [{k: v for k, v in self.review().items() if k != "at"}],
        }
        for name, data in cases.items():
            with self.subTest(name):
                code, out, err = run(reviews=self.write("r.json", json.dumps(data)))
                self.assertEqual((code, out), (1, b""))
                self.assertTrue(err.startswith("LADDER-INPUT: "), err)

    def test_deep_json_bad_utf8_and_size(self):
        for name, data in {"deep": ("[" * 100000 + "]" * 100000).encode(), "utf8": b'[{"pr": "\xff"}]',
                           "syntax": b"[{]", "huge-int": b'[{"pr": ' + b"9" * 5000 + b"}]"}.items():
            with self.subTest(name):
                code, out, err = run(reviews=self.write("r.json", data))
                self.assertEqual((code, out), (1, b""))
                self.assertTrue(err.startswith("LADDER-INPUT: "), err)
                self.assertNotIn("Traceback", err)
        with mock.patch.object(ladder, "MAX_REVIEWS_BYTES", 10):
            code, _, err = run()
        self.assertEqual(code, 1)
        self.assertIn("limit", err)

    def test_review_domains_and_ignored_ids(self):
        rows = [self.review(pr=i, node_ids=["ALN.A", "GDT.B", "bad id", ".X"]) for i in range(1, 6)]
        log = "".join(f"{i:040x}\x00X\x00{i}+bob@users.noreply.github.com\x002026-09-01T00:00:00Z\x00"
                      f"spec(GDT.N{i}): leaf\n" for i in range(3))
        rep = report(log=self.write("log.txt", log), reviews=self.write("r.json", json.dumps(rows)))
        self.assertEqual(names(rep), {"GDT": ["bob"]})
        self.assertEqual(rep["eligible"]["GDT"][0]["substantive_reviews"], 5)


class TestSafety(LadderCase):
    def _symlink(self, target, link):
        try:
            os.symlink(target, link)
        except (OSError, NotImplementedError):
            self.skipTest("cannot create symbolic links here")

    def test_symlinked_inputs_refused(self):
        self._symlink(LOG, self.tmp / "log-link.txt")
        self._symlink(REVIEWS, self.tmp / "rev-link.json")
        self._symlink(str(ladder.DEFAULT_CONFIG), self.tmp / "cfg-link.json")
        for kw, code_name in ((dict(log=str(self.tmp / "log-link.txt")), "LADDER-GIT"),
                              (dict(reviews=str(self.tmp / "rev-link.json")), "LADDER-INPUT"),
                              (dict(config=str(self.tmp / "cfg-link.json")), "LADDER-CONFIG")):
            with self.subTest(code_name):
                code, out, err = run(**kw)
                self.assertEqual((code, out), (1, b""))
                self.assertTrue(err.startswith(code_name + ": "), err)
                self.assertIn("symbolic link", err)

    def test_runs_offline_without_subprocess(self):
        def refuse(*a, **k):
            raise AssertionError("ladder.py must not start processes or open sockets")
        with mock.patch.object(subprocess, "Popen", refuse), mock.patch.object(socket, "socket", refuse), \
                mock.patch.object(socket, "create_connection", refuse), mock.patch.object(os, "system", refuse):
            code, out, err = run("--json")
        self.assertEqual((code, err), (0, ""))
        self.assertIn(b'"ALN"', out)

    def test_bad_now(self):
        for now in ("tomorrow", "", "0001-01-01T00:00:00Z"):
            with self.subTest(now):
                code, _, err = run(now=now)
                self.assertEqual(code, 1)
                self.assertTrue(err.startswith("LADDER-INPUT: "), err)

    def test_cli_errors_have_no_traceback(self):
        tool = str(ROOT / "tools" / "ladder.py")
        deep = self.write("deep.json", "[" * 100000 + "]" * 100000)
        for args, want in (([], 2),
                           (["--log", LOG, "--reviews", deep, "--now", NOW], 1),
                           (["--log", LOG, "--reviews", REVIEWS, "--now", NOW, "--json", "--md"], 2)):
            p = subprocess.run([sys.executable, tool] + args, capture_output=True, text=True)
            self.assertEqual(p.returncode, want, p.stderr)
            self.assertNotIn("Traceback", p.stderr)

    def test_cli_stdout_matches_main(self):
        tool = str(ROOT / "tools" / "ladder.py")
        p = subprocess.run([sys.executable, tool, "--log", LOG, "--reviews", REVIEWS, "--now", NOW, "--json"],
                           capture_output=True)
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(p.stdout, run("--json")[1])


class TestFixtures(unittest.TestCase):
    def test_log_fixture_is_up_to_date(self):
        spec = importlib.util.spec_from_file_location("ladder_make_fixtures", FIX / "make_fixtures.py")
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        self.assertEqual(mod.render(), (FIX / "git-log.txt").read_bytes())


if __name__ == "__main__":
    unittest.main()
