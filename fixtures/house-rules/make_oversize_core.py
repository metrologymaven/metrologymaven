"""Writes oversize-core.md: generated filler text, exactly 3 073 bytes (one over the core budget).

Fixture for spec node ECO.RULES.SRC AC-4. No third-party content. The expected size (3 073) is
hand-labelled from the spec, not computed by the code under test.
Run from anywhere:  python fixtures/house-rules/make_oversize_core.py
"""

from pathlib import Path

SIZE = 3073
OUT = Path(__file__).resolve().parent / "oversize-core.md"

text = "## Oversize core (test filler)\n\n"
n = 0
while True:
    line = f"Filler line {n:04d} for the core budget test.\n"
    if len(text) + len(line) > SIZE - 1:
        break
    text += line
    n += 1
text += "x" * (SIZE - 1 - len(text)) + "\n"
assert len(text.encode("utf-8")) == SIZE, len(text)
OUT.write_bytes(text.encode("utf-8"))
print(f"wrote {OUT.name}: {SIZE} bytes")
