"""Acceptance tests of spec node ECO.RULES.BLOCK (AC-1 .. AC-5) for tools/house_rules/block.py.

Expected values come from the independent reference fixtures/eco/rules/block/make_fixtures.py
(its own implementation; it does not import block.py) or are hand-labelled from the spec.
Run from the repository root:  python -m unittest discover -s tools/house_rules/tests -v
"""

import sys
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))

from house_rules import BlockError, RenderError, digest, find_blocks, is_intact, render  # noqa: E402

FIX = ROOT / "fixtures" / "eco" / "rules" / "block"
REV = "0123456789abcdef0123456789abcdef01234567"


def read(name):
    # newline="" keeps line endings exactly as stored (LF in the repository).
    with open(FIX / name, encoding="utf-8", newline="") as f:
        return f.read()


def fixture_text(name):
    """`<file>+crlf` is the file with every LF turned into CRLF."""
    if name.endswith("+crlf"):
        return read(name[: -len("+crlf")]).replace("\n", "\r\n")
    return read(name)


def data_rows(name):
    return [l for l in read(name).split("\n") if l and not l.startswith("#")]


class Fixtures(unittest.TestCase):
    def test_all_fixtures_match_reference(self):
        """Every fixture, LF and CRLF, parses as the reference says (AC-1, AC-3, AC-4 and more)."""
        rows = iter(data_rows("expected.txt"))
        files = 0
        for row in rows:
            f = row.split("\t")
            name = f[0]
            with self.subTest(fixture=name):
                if f[1] == "ok":
                    blocks = find_blocks(fixture_text(name))
                    self.assertEqual(len(blocks), int(f[2]))
                    for b in blocks:
                        e = next(rows).split("\t")
                        self.assertEqual(e[0], "block")
                        got = [b.id, b.source_repo, b.source_rev, b.sha256, b.computed_digest(),
                               str(b.begin_line), str(b.end_line), b.body.encode("utf-8").hex()]
                        self.assertEqual(got, e[1:])
                        self.assertEqual(is_intact(b), e[4] == e[5])
                else:
                    self.assertEqual(f[1], "error")
                    with self.assertRaises(BlockError) as cm:
                        find_blocks(fixture_text(name))
                    self.assertEqual((cm.exception.kind, cm.exception.line), (f[2], int(f[3])))
            files += 1
        self.assertEqual(files, 16)

    def test_ac1_one_block(self):
        blocks = find_blocks(read("one.md"))
        self.assertEqual(len(blocks), 1)
        b = blocks[0]
        self.assertEqual((b.id, b.source_repo, b.source_rev), ("core", "metrologymaven/fid-contrib", REV))
        self.assertEqual(b.sha256, "4ccba268100be81b10ea06b0f89908c3a7169ebdf98c0f7aee6b8db65f6573b8")
        self.assertEqual(digest(b.body), b.sha256)
        self.assertEqual((b.begin_line, b.end_line), (5, 13))

    def test_ac2_crlf_digest(self):
        lf = "## Rules\n\n1. one\n2. two\n"
        self.assertEqual(digest(lf.replace("\n", "\r\n")), digest(lf))
        body = find_blocks(read("one.md"))[0].body
        self.assertEqual(digest(body.replace("\n", "\r\n")), digest(body))
        self.assertNotEqual(digest("a\rb"), digest("a\nb"))  # a lone CR is content
        self.assertEqual(digest(""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")

    def test_ac3_unbalanced(self):
        for name, line in [("unbalanced.md", 2), ("stray-end.md", 3)]:
            with self.assertRaises(BlockError) as cm:
                find_blocks(read(name))
            self.assertEqual(cm.exception, BlockError("unbalanced", line))

    def test_ac4_nested(self):
        with self.assertRaises(BlockError) as cm:
            find_blocks(read("nested.md"))
        self.assertEqual(cm.exception, BlockError("nested", 4))
        self.assertEqual(str(cm.exception), "nested at line 4")

    def test_ac5_round_trip_200(self):
        """200 bodies from the reference's random.Random(42): same bytes, same fields."""
        rows = data_rows("render-cases-v1.txt")
        self.assertEqual(len(rows), 200)
        for i, row in enumerate(rows):
            bid, repo, rev, sha, body_hex, text_hex = row.split("\t")
            body = bytes.fromhex(body_hex).decode("utf-8")
            with self.subTest(case=i):
                text = render(bid, repo, rev, body)
                self.assertEqual(text, bytes.fromhex(text_hex).decode("utf-8"))
                (b,) = find_blocks(text)
                self.assertEqual((b.id, b.source_repo, b.source_rev, b.sha256, b.body), (bid, repo, rev, sha, body))
                self.assertTrue(b.is_intact())
                if "\r" not in body:
                    (c,) = find_blocks(text.replace("\n", "\r\n"))
                    self.assertEqual(c.sha256, sha)
                    self.assertTrue(c.is_intact())


class Rules(unittest.TestCase):
    """Hand-labelled from the spec and the documented fail-closed rules."""

    def test_render_refusals(self):
        cases = [
            (("A", "r", REV, ""), "bad-block-id", None),
            (("a", "r", REV, ""), "bad-block-id", None),
            (("a" * 33, "r", REV, ""), "bad-block-id", None),
            (("core", "", REV, ""), "bad-source-repo", None),
            (("core", "a@b", REV, ""), "bad-source-repo", None),
            (("core", "x" * 101, REV, ""), "bad-source-repo", None),
            (("core", "r", REV.upper(), ""), "bad-source-rev", None),
            (("core", "r", REV, "a\n  <!-- house-rules:x"), "marker-in-body", 2),
            (("core", "r", REV, "a\r"), "trailing-cr", None),
        ]
        for args, kind, line in cases:
            with self.subTest(args=args):
                with self.assertRaises(RenderError) as cm:
                    render(*args)
                self.assertEqual((cm.exception.kind, cm.exception.line), (kind, line))
        self.assertTrue(render("z" * 32, "x" * 100, REV, ""))

    def test_leading_bom_is_ignored(self):
        text = "﻿" + render("core", "r", REV, "body")
        (b,) = find_blocks(text)
        self.assertEqual((b.id, b.body, b.begin_line, b.end_line), ("core", "body", 1, 3))
        self.assertTrue(b.is_intact())

    def test_fail_closed_marker_lines(self):
        block = render("core", "r", REV, "body")
        begin, _, end, _ = block.split("\n")
        cases = {
            "fenced": "```\n" + block + "```\n",  # parsed: markers in a fence are still markers
            "indented begin": "  " + begin + "\nx\n" + end + "\n",
            "trailing space": begin + " \nx\n" + end + "\n",
            "tab after end": begin + "\nx\n" + end + "\t\n",
            "other keyword": "<!-- house-rules:middle -->\n",
            "bad hex": begin.replace("sha256=", "sha256=X")[:-5] + " -->\n",
            "double space": begin.replace(" source=", "  source=") + "\n" + end + "\n",
        }
        self.assertEqual(len(find_blocks(cases.pop("fenced"))), 1)
        lines = {"tab after end": 3}
        for name, text in cases.items():
            with self.subTest(case=name):
                with self.assertRaises(BlockError) as cm:
                    find_blocks(text)
                self.assertEqual((cm.exception.kind, cm.exception.line), ("bad-marker", lines.get(name, 1)))
        # Text that only mentions the prefix mid-line is not a marker line.
        self.assertEqual(find_blocks("see `<!-- house-rules:begin ...` above\n"), [])

    def test_id_mismatch_and_tamper(self):
        text = render("core", "r", REV, "a").replace("end block=core", "end block=rust")
        with self.assertRaises(BlockError) as cm:
            find_blocks(text)
        self.assertEqual(cm.exception, BlockError("id-mismatch", 3))
        (b,) = find_blocks(render("core", "r", REV, "a").replace("\na\n", "\nb\n"))
        self.assertFalse(is_intact(b))

    def test_performance_100kb(self):
        """Spec budget: under 10 ms for a 100 KB file. Asserted generously (< 200 ms) because CI
        runners are shared; the measured time is printed on failure."""
        block = render("core", "metrologymaven/fid-contrib", REV, "\n".join(["rule text line"] * 40))
        parts, size = [], 0
        while size < 100 * 1024:
            chunk = "some ordinary markdown text\n" + block
            parts.append(chunk)
            size += len(chunk)
        text = "".join(parts)
        best = None
        for _ in range(5):
            t = time.perf_counter()
            blocks = find_blocks(text)
            dt = time.perf_counter() - t
            best = dt if best is None else min(best, dt)
        self.assertTrue(all(b.is_intact() for b in blocks))
        self.assertLess(best, 0.200, f"best of 5: {best * 1000:.1f} ms")


if __name__ == "__main__":
    unittest.main()
