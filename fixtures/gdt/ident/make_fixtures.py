#!/usr/bin/env python3
"""Independent reference for GDT.CHAR.IDENT fixtures (stdlib only).

Writes, from the leaf's Design rules:

- imported_qpid.qif: a QIF 3.0 document with one flatness characteristic whose item element
  carries a native QPId, written in upper case so a test can see that nothing normalises it
  (AC-1).
- no_qpid_slot.qif: a QIF 3.0 document whose characteristic nominal carries no identity, only
  an unrelated string attribute in its Attributes list (AC-3). The test treats that element
  as one without a native QPId slot.
- expected.json: the imported QPId text, the identity the no-slot element must get from the
  test clock and seed, and UUIDv7 vectors for the determinism check (AC-5).

UUIDv7 here is built byte by byte from RFC 9562 section 5.7 (48-bit big-endian Unix
milliseconds, version nibble 7, 12 bits rand_a, variant 0b10, 62 bits rand_b), not by the
shifts the Rust code uses. The 74 random bits come from the leaf's seeded counter RNG: output
i (1-based) is the SplitMix64 finaliser of seed + i * 0x9e3779b97f4a7c15 mod 2**64; one
UUIDv7 takes two outputs a then b, and its random bits are the low 10 bits of a followed by
the 64 bits of b (rand_a = the top 12 of those 74 bits, rand_b = the low 62).

Which QIF element types have a native QPId slot, and the exact Attributes spelling used here
(AttributeStr with name and val), are [unverified] against the QIF 3.0 schema; the fixtures
only need to be well-formed XML of that shape. All numbers are integers written as decimal
strings in expected.json.

With --check it rebuilds every file in memory and fails if a committed file differs byte for
byte. All content is synthetic; CC0.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
M64 = (1 << 64) - 1
GAMMA = 0x9E3779B97F4A7C15

IMPORTED_QPID = "4F2A9C3E-1B7D-4E8A-9C21-7D3B5E6F8A10"
OTHER_ATTR = ("fx:note", "synthetic test attribute")
NO_SLOT_CLOCK_MS = 1_763_500_000_000
NO_SLOT_SEED = 20_261_119


def splitmix(seed, i):
    z = (seed + i * GAMMA) & M64
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return z ^ (z >> 31)


def uuid7(ms, r74):
    assert 0 <= ms < (1 << 48) and 0 <= r74 < (1 << 74)
    rand_a = r74 >> 62
    rand_b = r74 & ((1 << 62) - 1)
    b = bytearray(16)
    b[0:6] = ms.to_bytes(6, "big")
    b[6] = 0x70 | (rand_a >> 8)
    b[7] = rand_a & 0xFF
    tail = rand_b.to_bytes(8, "big")  # top two bits are zero (62-bit value)
    b[8] = 0x80 | tail[0]
    b[9:16] = tail[1:8]
    h = b.hex()
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def draw(ms, seed, count):
    out = []
    i = 0
    for _ in range(count):
        a = splitmix(seed, i + 1)
        b = splitmix(seed, i + 2)
        i += 2
        out.append(uuid7(ms, ((a & 0x3FF) << 64) | b))
    return out


CASES = [
    ("zero_seed", 0, 0, 2),
    ("typical", 1_700_000_000_000, 42, 3),
    ("max_clock", (1 << 48) - 1, 7, 1),
    ("max_seed", 1_763_500_000_000, M64, 2),
    ("no_slot", NO_SLOT_CLOCK_MS, NO_SLOT_SEED, 1),
]


def qif_imported():
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!-- GDT.CHAR.IDENT AC-1 fixture: synthetic, CC0. Written by make_fixtures.py. -->
<QIFDocument xmlns="http://qifstandards.org/xsd/qif3" versionQIF="3.0.0" idMax="3">
  <Characteristics>
    <CharacteristicDefinitions n="1">
      <FlatnessCharacteristicDefinition id="1">
        <ToleranceValue>0.05</ToleranceValue>
      </FlatnessCharacteristicDefinition>
    </CharacteristicDefinitions>
    <CharacteristicNominals n="1">
      <FlatnessCharacteristicNominal id="2">
        <CharacteristicDefinitionId>1</CharacteristicDefinitionId>
      </FlatnessCharacteristicNominal>
    </CharacteristicNominals>
    <CharacteristicItems n="1">
      <FlatnessCharacteristicItem id="3">
        <QPId>{IMPORTED_QPID}</QPId>
        <CharacteristicNominalId>2</CharacteristicNominalId>
      </FlatnessCharacteristicItem>
    </CharacteristicItems>
  </Characteristics>
</QIFDocument>
"""


def qif_no_slot():
    name, val = OTHER_ATTR
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!-- GDT.CHAR.IDENT AC-3 fixture: synthetic, CC0. Written by make_fixtures.py. -->
<QIFDocument xmlns="http://qifstandards.org/xsd/qif3" versionQIF="3.0.0" idMax="2">
  <Characteristics>
    <CharacteristicDefinitions n="1">
      <FlatnessCharacteristicDefinition id="1">
        <ToleranceValue>0.05</ToleranceValue>
      </FlatnessCharacteristicDefinition>
    </CharacteristicDefinitions>
    <CharacteristicNominals n="1">
      <FlatnessCharacteristicNominal id="2">
        <Attributes n="1">
          <AttributeStr name="{name}" val="{val}"/>
        </Attributes>
        <CharacteristicDefinitionId>1</CharacteristicDefinitionId>
      </FlatnessCharacteristicNominal>
    </CharacteristicNominals>
  </Characteristics>
</QIFDocument>
"""


def expected():
    doc = {
        "imported_qpid": IMPORTED_QPID,
        "no_slot": {
            "clock_ms": str(NO_SLOT_CLOCK_MS),
            "seed": str(NO_SLOT_SEED),
            "identity": draw(NO_SLOT_CLOCK_MS, NO_SLOT_SEED, 1)[0],
            "kept_attribute": {"name": OTHER_ATTR[0], "value": OTHER_ATTR[1]},
        },
        "v7_cases": [
            {"name": n, "clock_ms": str(ms), "seed": str(seed), "uuids": draw(ms, seed, k)}
            for (n, ms, seed, k) in CASES
        ],
    }
    return json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=True) + "\n"


FILES = {
    "imported_qpid.qif": qif_imported,
    "no_qpid_slot.qif": qif_no_slot,
    "expected.json": expected,
}


def main(argv):
    check = "--check" in argv[1:]
    bad = 0
    for name, make in FILES.items():
        data = make().encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(data)
    if bad:
        print("run: python fixtures/gdt/ident/make_fixtures.py", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
