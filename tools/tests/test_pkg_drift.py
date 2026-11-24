"""Tier-5 tests for fixtures/pkg/check_spec_drift.py blind-spot fixes."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "fixtures" / "pkg" / "check_spec_drift.py"


def load_drift_module():
    spec = importlib.util.spec_from_file_location("check_spec_drift", SCRIPT)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


class SpecDriftTier5Test(unittest.TestCase):
    def setUp(self) -> None:
        self.mod = load_drift_module()
        self.errors_text = "\n".join(p.read_text(encoding="utf-8") for p in self.mod.ERROR_FILES)
        self.reader_text = "\n".join(p.read_text(encoding="utf-8") for p in self.mod.READER_FILES)
        self.manifest_text = self.mod.MANIFEST_FILE.read_text(encoding="utf-8")
        self.spec_text = self.mod.SPEC.read_text(encoding="utf-8")
        self.spec = self.mod.spec_items(self.spec_text)

    def test_live_repo_passes_self_test(self) -> None:
        code = self.mod.code_items(self.errors_text, self.reader_text, self.manifest_text)
        self.assertEqual(self.mod.compare(code, self.spec), [])
        self.mod.self_test(self.errors_text, self.reader_text, self.manifest_text, self.spec_text, self.spec)

    def test_multiline_nested_paren_variable_field_and_table_swap_caught(self) -> None:
        # 1. Multiline limit(...) and malformed(...)
        r1 = self.reader_text + '\nlimit(\n    "extra_limit",\n    "d",\n)\nmalformed(\n    "extra_malf",\n    "d",\n)\n'
        c1 = self.mod.code_items(self.errors_text, r1, self.manifest_text)
        diff1 = self.mod.compare(c1, self.spec)
        self.assertTrue(any("reasons:pkg.limit: `extra_limit`" in d for d in diff1), diff1)
        self.assertTrue(any("reasons:pkg.malformed: `extra_malf`" in d for d in diff1), diff1)

        # 2. Nested parens in first argument of ReadError::mismatch
        r2 = self.reader_text + '\nReadError::mismatch(foo.bar().as_str(),\n    "extra_mismatch")\n'
        c2 = self.mod.code_items(self.errors_text, r2, self.manifest_text)
        diff2 = self.mod.compare(c2, self.spec)
        self.assertTrue(any("reasons:pkg.digest_mismatch: `extra_mismatch`" in d for d in diff2), diff2)

        # 3. Variable-backed field inside Manifest::to_value
        m3 = self.manifest_text.replace(
            '("members", CValue::Arr(members)),',
            '("members", CValue::Arr(members)), ("extra_var_field", some_val),',
            1,
        )
        c3 = self.mod.code_items(self.errors_text, self.reader_text, m3)
        diff3 = self.mod.compare(c3, self.spec)
        self.assertTrue(any("fields:manifest: `extra_var_field`" in d for d in diff3), diff3)

        # 4. Cross-table swap between manifest and statement in docs/result-package/v1.md
        swapped = self.spec_text.replace("| `created_by` |", "| `result_count` |", 1).replace(
            "| `result_count` | string | Number", "| `created_by` | string | Number", 1
        )
        diff4 = self.mod.compare(
            self.mod.code_items(self.errors_text, self.reader_text, self.manifest_text),
            self.mod.spec_items(swapped),
        )
        self.assertTrue(any("fields:manifest: `created_by`" in d for d in diff4), diff4)
        self.assertTrue(any("fields:statement: `result_count`" in d for d in diff4), diff4)

        # 5. Dynamic MEMBER_DIRS array count [ &str; 7 ]
        m5 = self.manifest_text.replace(
            'MEMBER_DIRS: [&str; 6] = ["interp"',
            'MEMBER_DIRS: [&str; 7] = ["extra_dir", "interp"',
            1,
        )
        self.mod.self_test(self.errors_text, self.reader_text, m5, self.spec_text, self.spec)


if __name__ == "__main__":
    unittest.main()
