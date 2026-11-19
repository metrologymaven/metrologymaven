#!/usr/bin/env python3
"""Independent reference for VIZ.RENDER.SCENE.IDTAB AC-1 and AC-4 (project-authored, CC0).

Writes, next to this script:

- drawables.txt: 800 drawables (500 entity, 300 derived) in registration order, each line
  `<expected render id> entity <uuid>` or `<expected render id> derived <uuid> <op>`;
- by_entity.txt: one line per entity, `<uuid> <render id> ...`, the expected answer of the
  entity-to-render lookup (the entity's own render ID and those of every derived drawable whose
  source is the entity), ascending, entities sorted by UUID text.

The expected render IDs follow the leaf's rules, not the Rust code: IDs are dense u32 values
starting at 1 in registration order (REQ-001), and the reverse lookup returns every live render
ID bound to the entity or derived from it, ascending (REQ-004, AC-4).

Seed 21 drives Python's own Mersenne Twister (`random.Random(21)`). Every (source, op) pair is
distinct, as the table's one-live-render-ID-per-identity rule requires, and the script asserts
that at least one entity has exactly three derived drawables (AC-4).

Run:  python fixtures/viz/scene-ids/make_fixture.py          (write)
      python fixtures/viz/scene-ids/make_fixture.py --check  (compare, exit 1 on drift)
"""

import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SEED = 21
ENTITIES = 500
DERIVED = 300
# View-operation names in their pinned code order (NODE.md Design: Whisker = 1 .. Proxy = 6).
OPS = ["whisker", "label", "leader", "ghost", "clip_cap", "proxy"]


def uuid_v4(rng):
    """A random RFC 9562 version-4 UUID in hyphenated lowercase text."""
    n = rng.getrandbits(128)
    n &= ~(0xF << 76)
    n |= 0x4 << 76
    n &= ~(0x3 << 62)
    n |= 0x2 << 62
    h = f"{n:032x}"
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def build():
    rng = random.Random(SEED)
    entities = []
    seen = set()
    while len(entities) < ENTITIES:
        u = uuid_v4(rng)
        if u not in seen:
            seen.add(u)
            entities.append(u)

    pairs = rng.sample(range(ENTITIES * len(OPS)), DERIVED)
    derived = [(entities[p // len(OPS)], OPS[p % len(OPS)]) for p in pairs]

    drawables = [("entity", u, None) for u in entities] + [("derived", u, op) for u, op in derived]
    rng.shuffle(drawables)

    lines = [
        "# VIZ.RENDER.SCENE.IDTAB AC-1 fixture: written by make_fixture.py (seed 21), CC0.\n",
        "# <expected render id> entity <uuid> | <expected render id> derived <uuid> <op>\n",
    ]
    by_entity = {u: [] for u in entities}
    for i, (kind, u, op) in enumerate(drawables):
        rid = i + 1
        by_entity[u].append(rid)
        if kind == "entity":
            lines.append(f"{rid} entity {u}\n")
        else:
            lines.append(f"{rid} derived {u} {op}\n")

    counts = [len(v) - 1 for v in by_entity.values()]
    assert counts.count(3) >= 1, "AC-4 needs an entity with exactly three derived drawables"
    assert len(lines) - 2 == ENTITIES + DERIVED

    rev = [
        "# VIZ.RENDER.SCENE.IDTAB AC-1/AC-4 expected reverse lookups: written by make_fixture.py, CC0.\n",
        "# <uuid> <render id> ... (ascending; own entity drawable plus derived drawables)\n",
    ]
    for u in sorted(by_entity):
        ids = sorted(by_entity[u])
        rev.append(u + " " + " ".join(str(r) for r in ids) + "\n")
    return {"drawables.txt": "".join(lines), "by_entity.txt": "".join(rev)}


def main():
    check = "--check" in sys.argv[1:]
    bad = False
    for name, text in build().items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{name}: differs from make_fixture.py output", file=sys.stderr)
                bad = True
        else:
            path.write_bytes(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
