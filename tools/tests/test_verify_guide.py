"""Tests of tools/verify_guide.py, the guide-follow check of spec node ECO.REL.SIGN.GUIDE (AC-3).

Expected extractions are hand-written from the fenced-block rules, not produced by running the
tool. The full run (untouched release passes, one changed byte fails) is exercised for the
current OS's tag where its shell exists; the CI job `verify-guide` runs all three tags.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import shutil
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import verify_guide as vg  # noqa: E402

DOC = """# Guide

```sh verify-linux
echo one
```

```text
not run
```

~~~~ powershell verify-windows
Write-Output two
```
still inside the tilde fence
~~~~

```sh verify-linux
echo three
```
"""


class Extract(unittest.TestCase):
    def test_tagged_blocks_in_order(self):
        self.assertEqual(vg.extract(DOC, "verify-linux"), ["echo one\n", "echo three\n"])
        self.assertEqual(vg.script_for(DOC, "verify-linux"), "echo one\necho three\n")

    def test_other_fence_kind_does_not_close(self):
        self.assertEqual(
            vg.extract(DOC, "verify-windows"), ["Write-Output two\n```\nstill inside the tilde fence\n"]
        )

    def test_missing_tag_is_an_error(self):
        with self.assertRaises(vg.GuideError):
            vg.script_for(DOC, "verify-macos")

    def test_tag_must_be_a_whole_word(self):
        self.assertEqual(vg.extract("```sh verify-linux-old\necho x\n```\n", "verify-linux"), [])

    def test_unclosed_block_is_an_error(self):
        with self.assertRaises(vg.GuideError):
            vg.extract("```sh verify-linux\necho x\n", "verify-linux")

    def test_verify_md_has_every_tag(self):
        text = (ROOT / "VERIFY.md").read_text(encoding="utf-8")
        for tag in vg.TAGS:
            self.assertTrue(vg.script_for(text, tag).strip(), tag)


class Tamper(unittest.TestCase):
    def test_changes_exactly_one_byte_of_the_first_listed_file(self):
        src = ROOT / "fixtures" / "rel" / "guide" / "release"
        with tempfile.TemporaryDirectory() as tmp:
            copy = Path(tmp) / "r"
            shutil.copytree(src, copy)
            name = vg.tamper(copy)
            self.assertEqual(name, "fid-0.0.0-fixture+src.tar.gz")
            a, b = (src / name).read_bytes(), (copy / name).read_bytes()
            self.assertEqual(len(a), len(b))
            self.assertEqual(sum(x != y for x, y in zip(a, b)), 1)


class Follow(unittest.TestCase):
    def test_linux_guide_on_fixture(self):
        if not sys.platform.startswith("linux") or shutil.which("sha256sum") is None:
            self.skipTest("Linux only")
        vg.follow(ROOT / "VERIFY.md", "verify-linux", ROOT / "fixtures" / "rel" / "guide" / "release")


if __name__ == "__main__":
    unittest.main()
