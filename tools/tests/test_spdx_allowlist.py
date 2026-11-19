"""Acceptance tests of spec node ECO.RULES.SPDX (AC-1 .. AC-4) for house-rules/spdx-allowlist.toml.

Also checks that the dependency part of the allowlist (`licences`, `exceptions`) equals the
`[licenses] allow` list of deny.toml (maintainer decision 2026-11-18), failing on drift in
either direction. Expected values are hand-labelled from the spec; the negative fixture
fixtures/spdx/bad-gpl.toml is a hand-made copy of the list with GPL-3.0-only added.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import re
import unittest
from pathlib import Path

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None

ROOT = Path(__file__).resolve().parents[2]
ALLOWLIST = ROOT / "house-rules" / "spdx-allowlist.toml"
BAD_GPL = ROOT / "fixtures" / "spdx" / "bad-gpl.toml"
DENY = ROOT / "deny.toml"
MANIFEST = ROOT / "house-rules" / "manifest.toml"

LIST_KEYS = ("licences", "exceptions", "asset_licences", "asset_paths")
FORBIDDEN = re.compile(r"^(A|L)?GPL-|^SSPL-|-NC-|^CC-BY-NC")
SPDX_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9.+-]*")

SKIP = tomllib is None
SKIP_MSG = "ECO.RULES.SPDX tests need tomllib (Python >= 3.11); no JSON twin of the allowlist exists"


def load(path):
    with open(path, "rb") as f:
        return tomllib.load(f)


def schema_problems(data):
    """REQ-005 / AC-1: the four list keys hold string lists, `basis` covers exactly the IDs used."""
    out = []
    for k in LIST_KEYS:
        v = data.get(k)
        if not isinstance(v, list) or not all(isinstance(s, str) and s for s in v):
            out.append(f"`{k}` must be a list of non-empty strings")
        elif len(v) != len(set(v)):
            out.append(f"`{k}` has duplicate entries")
    basis = data.get("basis")
    if not isinstance(basis, dict):
        return out + ["`[basis]` table missing"]
    if out:
        return out
    ids = set(data["licences"]) | set(data["exceptions"]) | set(data["asset_licences"])
    for i in sorted(ids):
        if not isinstance(basis.get(i), str) or not basis[i].strip():
            out.append(f"`{i}` has no basis note")
        if not SPDX_ID.fullmatch(i):
            out.append(f"`{i}` is not a single SPDX ID")
    for i in sorted(set(basis) - ids):
        out.append(f"basis note for `{i}`, which no list uses")
    return out


def forbidden_ids(data):
    """REQ-002 / AC-2: IDs in the lists that match the copyleft / non-commercial pattern."""
    keys = ("licences", "exceptions", "asset_licences")
    return sorted({i for k in keys for i in data.get(k, []) if FORBIDDEN.search(i)})


def deny_sets(deny):
    """Split deny.toml's `[licenses] allow` into (licence IDs, exception IDs)."""
    lic, exc = set(), set()
    for expr in deny["licenses"]["allow"]:
        parts = expr.split(" WITH ")
        if len(parts) > 2:
            raise ValueError(f"unexpected deny.toml entry {expr!r}")
        lic.add(parts[0].strip())
        if len(parts) == 2:
            exc.add(parts[1].strip())
    return lic, exc


def drift(allow, deny):
    """Differences between the allowlist and deny.toml; empty when they agree."""
    d_lic, d_exc = deny_sets(deny)
    a_lic, a_exc = set(allow["licences"]), set(allow["exceptions"])
    out = []
    for i in sorted(a_lic - d_lic):
        out.append(f"licence {i} is in spdx-allowlist.toml but not in deny.toml")
    for i in sorted(d_lic - a_lic):
        out.append(f"licence {i} is in deny.toml but not in spdx-allowlist.toml")
    for i in sorted(a_exc - d_exc):
        out.append(f"exception {i} is in spdx-allowlist.toml but not in deny.toml")
    for i in sorted(d_exc - a_exc):
        out.append(f"exception {i} is in deny.toml but not in spdx-allowlist.toml")
    return out


@unittest.skipIf(SKIP, SKIP_MSG)
class Schema(unittest.TestCase):
    """AC-1, REQ-001 (basis per entry), REQ-005."""

    def test_parses_with_four_keys_and_basis(self):
        data = load(ALLOWLIST)
        for k in LIST_KEYS:
            self.assertIn(k, data)
        self.assertEqual(schema_problems(data), [])

    def test_every_licence_has_basis(self):
        data = load(ALLOWLIST)
        for i in data["licences"]:
            self.assertIn(i, data["basis"])

    def test_schema_guard_catches_missing_basis_and_key(self):
        data = load(ALLOWLIST)
        no_basis = dict(data, basis={k: v for k, v in data["basis"].items() if k != "ISC"})
        self.assertEqual(schema_problems(no_basis), ["`ISC` has no basis note"])
        no_key = {k: v for k, v in data.items() if k != "asset_paths"}
        self.assertEqual(schema_problems(no_key), ["`asset_paths` must be a list of non-empty strings"])

    def test_listed_in_manifest(self):
        files = [d["file"] for d in load(MANIFEST).get("data", [])]
        self.assertIn("spdx-allowlist.toml", files)


@unittest.skipIf(SKIP, SKIP_MSG)
class Copyleft(unittest.TestCase):
    """AC-2, REQ-002."""

    def test_allowlist_has_no_forbidden_id(self):
        self.assertEqual(forbidden_ids(load(ALLOWLIST)), [])

    def test_bad_gpl_fixture_is_rejected(self):
        bad = load(BAD_GPL)
        self.assertEqual(schema_problems(bad), [])  # only the copyleft guard may fire
        self.assertEqual(forbidden_ids(bad), ["GPL-3.0-only"])

    def test_bad_gpl_fixture_is_the_list_plus_gpl(self):
        good, bad = load(ALLOWLIST), load(BAD_GPL)
        self.assertEqual(bad["licences"], good["licences"] + ["GPL-3.0-only"])
        for k in ("exceptions", "asset_licences", "asset_paths"):
            self.assertEqual(bad[k], good[k])

    def test_pattern_hand_labelled(self):
        hits = ["GPL-2.0-only", "LGPL-2.1-or-later", "AGPL-3.0-only", "SSPL-1.0", "CC-BY-NC-4.0",
                "CC-BY-NC-SA-4.0", "CC-BY-SA-NC-1.0"]
        misses = ["MIT", "Apache-2.0", "CC-BY-4.0", "CC0-1.0", "OFL-1.1", "MPL-2.0", "BSD-3-Clause"]
        for i in hits:
            self.assertRegex(i, FORBIDDEN)
        for i in misses:
            self.assertNotRegex(i, FORBIDDEN)


@unittest.skipIf(SKIP, SKIP_MSG)
class Contents(unittest.TestCase):
    """AC-3, AC-4, REQ-003."""

    def test_asset_licences(self):
        assets = load(ALLOWLIST)["asset_licences"]
        for i in ("CC-BY-4.0", "CC0-1.0", "OFL-1.1"):
            self.assertIn(i, assets)

    def test_project_licences(self):
        lic = load(ALLOWLIST)["licences"]
        self.assertIn("MIT", lic)
        self.assertIn("Apache-2.0", lic)


@unittest.skipIf(SKIP, SKIP_MSG)
class AgreesWithDenyToml(unittest.TestCase):
    """Maintainer decision 2026-11-18: no wider (and no narrower) than deny.toml."""

    def test_allowlist_equals_deny_toml(self):
        self.assertEqual(drift(load(ALLOWLIST), load(DENY)), [])

    def test_drift_detected_when_allowlist_is_wider(self):
        allow = load(ALLOWLIST)
        wider = dict(allow, licences=allow["licences"] + ["0BSD"], exceptions=allow["exceptions"] + ["X-exception"])
        self.assertEqual(drift(wider, load(DENY)), [
            "licence 0BSD is in spdx-allowlist.toml but not in deny.toml",
            "exception X-exception is in spdx-allowlist.toml but not in deny.toml",
        ])

    def test_drift_detected_when_deny_toml_is_wider(self):
        deny = load(DENY)
        wider = {"licenses": dict(deny["licenses"], allow=deny["licenses"]["allow"] + ["MIT-0", "MIT WITH Y-exception"])}
        self.assertEqual(drift(load(ALLOWLIST), wider), [
            "licence MIT-0 is in deny.toml but not in spdx-allowlist.toml",
            "exception Y-exception is in deny.toml but not in spdx-allowlist.toml",
        ])

    def test_drift_detected_when_entry_removed(self):
        allow = load(ALLOWLIST)
        narrower = dict(allow, licences=[i for i in allow["licences"] if i != "Zlib"], exceptions=[])
        self.assertEqual(drift(narrower, load(DENY)), [
            "licence Zlib is in deny.toml but not in spdx-allowlist.toml",
            "exception LLVM-exception is in deny.toml but not in spdx-allowlist.toml",
        ])


if __name__ == "__main__":
    unittest.main()
