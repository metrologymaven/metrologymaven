#!/usr/bin/env python3
"""Generate the IO.QIF.GEN.PIN fixtures (AC-1..AC-4). CC-BY-4.0 project data.

The XSD-like files below are tiny, hand-written and synthetic: they are NOT DMSC QIF
files. Sizes and SHA-256 digests are computed here with Python's hashlib, independently
of the Rust crate, and written into each case's manifest and into expected.toml.

  ok/       three pinned XSDs + licence file + matching manifest      (AC-1)
  changed/  as ok/, but one byte of b.xsd differs (same size)          (AC-2)
  extra/    as ok/, plus an unlisted c.xsd                             (AC-3)

Run:  python fixtures/io/qif/gen/pin/make_fixtures.py
"""

import hashlib
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent

FILES = {
    "a.xsd": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:fid:test:pin">\n'
        '  <xs:include schemaLocation="b.xsd"/>\n'
        '  <xs:include schemaLocation="lib/units.xsd"/>\n'
        '  <xs:element name="Root" type="RootType"/>\n'
        "</xs:schema>\n"
    ),
    "b.xsd": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:fid:test:pin">\n'
        '  <xs:complexType name="RootType">\n'
        "    <xs:sequence>\n"
        '      <xs:element name="Length" type="LengthType"/>\n'
        "    </xs:sequence>\n"
        "  </xs:complexType>\n"
        "</xs:schema>\n"
    ),
    "lib/units.xsd": (
        '<?xml version="1.0" encoding="UTF-8"?>\n'
        '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema" targetNamespace="urn:fid:test:pin">\n'
        '  <xs:simpleType name="LengthType">\n'
        '    <xs:restriction base="xs:double"/>\n'
        "  </xs:simpleType>\n"
        "</xs:schema>\n"
    ),
}
LICENCE_PATH = "LICENSE.txt"
LICENCE = "Synthetic licence text for the IO.QIF.GEN.PIN test fixtures (CC-BY-4.0).\n"
EXTRA = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"/>\n'
)
SOURCE_URL = "https://example.org/fid-test/pin-fixtures.git"
SOURCE_REV = "0123456789abcdef0123456789abcdef01234567"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def manifest(files: dict) -> bytes:
    lic = LICENCE.encode()
    out = [
        "# Synthetic test manifest (IO.QIF.GEN.PIN fixtures); written by make_fixtures.py.",
        "manifest_version = 1",
        f'licence_path = "{LICENCE_PATH}"',
        f'licence_sha256 = "{sha(lic)}"',
        "",
        "[source]",
        f'url = "{SOURCE_URL}"',
        f'rev = "{SOURCE_REV}"',
    ]
    for path in sorted(files):
        data = files[path].encode()
        out += ["", "[[file]]", f'path = "{path}"', f"bytes = {len(data)}", f'sha256 = "{sha(data)}"', 'role = "xsd"']
    return ("\n".join(out) + "\n").encode()


def case(name: str, files: dict, extra: dict) -> None:
    root = HERE / name
    if root.exists():
        shutil.rmtree(root)
    for path, text in {**files, **extra}.items():
        write(root / path, text.encode())
    write(root / LICENCE_PATH, LICENCE.encode())
    write(root / "qif-xsd.manifest.toml", manifest(files))


def main() -> None:
    changed = dict(FILES)
    original = FILES["b.xsd"]
    # One byte differs, size unchanged: "Length" -> "Lenhth" in the element name.
    i = original.index("Length")
    changed_b = original[: i + 3] + "h" + original[i + 4 :]
    assert len(changed_b) == len(original) and changed_b != original

    case("ok", FILES, {})
    # changed/: manifest pins the ORIGINAL b.xsd, the file on disk is the changed one.
    case("changed", FILES, {})
    write(HERE / "changed" / "b.xsd", changed_b.encode())
    case("extra", FILES, {"c.xsd": EXTRA})

    lines = [
        "# Expected values for the IO.QIF.GEN.PIN tests; written by make_fixtures.py (hashlib).",
        f'licence_sha256 = "{sha(LICENCE.encode())}"',
        f'changed_b_expected = "{sha(original.encode())}"',
        f'changed_b_actual = "{sha(changed_b.encode())}"',
        f"changed_b_bytes = {len(original.encode())}",
        "",
        "[ok]",
    ]
    for path in sorted(FILES):
        lines.append(f'"{path}" = "{sha(FILES[path].encode())}"')
    write(HERE / "expected.toml", ("\n".join(lines) + "\n").encode())


if __name__ == "__main__":
    main()
