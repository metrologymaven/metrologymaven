#!/usr/bin/env python3
"""Independent fixture generator for IO.DMIS.DIAL (stdlib only; CC-BY-4.0).

Builds synthetic DMIS dialect profile fixtures under `fixtures/io/dmis/dial/`:
  - `test-vendor.toml`: valid profile adding `VNDX/ON` (`test-vendor.vndx_on`) and
    relaxation `allow_lowercase_labels` (AC-1, AC-2, AC-4)
  - `uses_vndx.dmi`: synthetic DMIS file containing `VNDX/ON` + a standard `FEAT/POINT` (AC-2)
  - `redefines_feat.toml`: invalid profile attempting to redefine base major `FEAT` (AC-3)
  - `bad_shape.toml`: invalid profile with an unknown shape kind
  - `README.md`: provenance and CC-BY-4.0 clean-room note

Usage:
  python3 fixtures/io/dmis/dial/make_fixtures.py          # write files
  python3 fixtures/io/dmis/dial/make_fixtures.py --check  # verify byte-for-byte match
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

TEST_VENDOR_TOML = """\
schema = "fid.dmis-dialect"
schema_version = 1
id = "test-vendor"
version = "1.0.0"
base = "dmis-5.3"
title = "Synthetic Test Vendor DMIS Dialect"
source_note = "Synthetic clean-room test fixture (CC-BY-4.0); no vendor text copied."
relaxations = ["allow_lowercase_labels"]

[[descriptors]]
class = "test-vendor.vndx_on"
major = "VNDX"
label_kind = "none"
shape = { kind = "keyword", value = "ON" }
"""

USES_VNDX_DMI = """\
DMISMN/'dialect-test',05.3
VNDX/ON
F(P1)=FEAT/POINT,CART,1.0,2.0,3.0,0.0,0.0,1.0
ENDFIL
"""

REDEFINES_FEAT_TOML = """\
schema = "fid.dmis-dialect"
schema_version = 1
id = "bad-vendor"
version = "1.0.0"
base = "dmis-5.3"
title = "Invalid Dialect Redefining FEAT"
source_note = "Synthetic negative test fixture (CC-BY-4.0)."
relaxations = []

[[descriptors]]
class = "bad-vendor.feat_override"
major = "FEAT"
label_kind = "required:F"
shape = { kind = "keyword", value = "POINT" }
"""

BAD_SHAPE_TOML = """\
schema = "fid.dmis-dialect"
schema_version = 1
id = "bad-shape"
version = "1.0.0"
base = "dmis-5.3"
title = "Invalid Dialect With Bad Shape"
source_note = "Synthetic negative test fixture (CC-BY-4.0)."

[[descriptors]]
class = "bad-shape.vndx_bad"
major = "VNDY"
label_kind = "none"
shape = { kind = "not_a_real_shape_kind" }
"""

README_MD = """\
# IO.DMIS.DIAL test fixtures

- **Spec node:** `IO.DMIS.DIAL` (`fid-io-dmis::dial`, `import.dmis_list_dialects@1`)
- **License:** CC-BY-4.0 (synthetic clean-room profiles and DMIS fragment; zero vendor text copied)
- **Generator:** `python3 fixtures/io/dmis/dial/make_fixtures.py` (verify with `--check`)

## Files

- `test-vendor.toml` (AC-1, AC-2, AC-4): valid `fid.dmis-dialect` v1 profile (`test-vendor` `1.0.0`,
  `base = "dmis-5.3"`, relaxation `allow_lowercase_labels`, extra statement `VNDX/ON` -> `test-vendor.vndx_on`).
- `uses_vndx.dmi` (AC-2): minimal DMIS snippet containing `VNDX/ON` and a standard `FEAT/POINT`.
- `redefines_feat.toml` (AC-3): negative fixture redefining base DMIS major word `FEAT` (`base_redefinition`).
- `bad_shape.toml`: negative fixture with an invalid shape descriptor (`shape_compile_error`).
"""


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        HERE / "test-vendor.toml": TEST_VENDOR_TOML,
        HERE / "uses_vndx.dmi": USES_VNDX_DMI,
        HERE / "redefines_feat.toml": REDEFINES_FEAT_TOML,
        HERE / "bad_shape.toml": BAD_SHAPE_TOML,
        HERE / "README.md": README_MD,
    }
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_text(encoding="utf-8") != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    HERE.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
