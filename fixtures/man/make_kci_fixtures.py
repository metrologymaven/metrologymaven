#!/usr/bin/env python3
"""Generate the KRN.MAN.KCI fixtures (CC-BY-4.0 project data) independently of the Rust code.

Every expected value here comes from Python's standard library (hashlib, json, tomllib),
never from the Rust implementation.

- fixtures/man/sha256-cavp-v1/short-msg.rsp   CAVP-format byte-oriented short messages (0..64 bytes)
- fixtures/man/sha256-cavp-v1/long-msg.rsp    CAVP-format byte-oriented long messages (64 messages)
- fixtures/man/sha256-cavp-v1/fips180-examples.txt  FIPS 180-4 example messages and published digests
- fixtures/man/canon-json-v1.txt              canonical JSON writer cases (strings, arrays, objects)
- fixtures/man/kci-calls-v1.txt               20 kernel calls of test.echo.identity@1.0.0 with golden results
- fixtures/man/card-digests-v1.txt            canonical card bytes digests of fixtures/man/cards-v1 (REQ-KRN.MAN.CARD-005)

The *.rsp files use the NIST CAVP SHA256ShortMsg/LongMsg layout and message lengths, but the
messages are pseudo-random bytes from a fixed seed and the digests come from hashlib: they are
not the NIST files themselves. The published FIPS 180-4 example digests and a few NIST CAVP
sample vectors are hard-coded below and checked against hashlib before anything is written.

Run:          python fixtures/man/make_kci_fixtures.py
Check (CI):   python fixtures/man/make_kci_fixtures.py --check   (writes nothing; exits 1 on any stale file)
"""

import hashlib
import json
import random
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).parent
CHECK = "--check" in sys.argv[1:]
STALE = []


def write(path, lines):
    data = ("\n".join(lines) + "\n").encode("utf-8")
    if CHECK:
        if not path.is_file() or path.read_bytes() != data:
            STALE.append(path)
        return
    path.parent.mkdir(exist_ok=True)
    path.write_bytes(data)


def sha(b):
    return hashlib.sha256(b).hexdigest()


# --- SHA-256 -------------------------------------------------------------------------------
# FIPS 180-4 examples (NIST "Example Algorithms" SHA-256 sheets) and NIST CAVP sample vectors.
EXAMPLES = [
    ("fips-abc", b"abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
    ("fips-448", b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
     "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"),
    ("fips-896", b"abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmnhijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
     "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1"),
    ("empty", b"", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
    ("cavp-len8", bytes.fromhex("d3"), "28969cdfa74a12c82f3bad960b0b000aca2ac329deea5c2328ebc6f2ba9802c1"),
    ("cavp-len16", bytes.fromhex("11af"), "5ca7133fa735326081558ac312c620eeca9970d1e70a4b95533d956f072d1f98"),
]
MILLION_A = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
for name, msg, md in EXAMPLES:
    assert sha(msg) == md, name
assert sha(b"a" * 1_000_000) == MILLION_A

lines = ["# FIPS 180-4 example messages and NIST CAVP samples. Format: <name><TAB><msg hex><TAB><digest hex>.",
         "# Special line: million-a<TAB>repeat:61:1000000<TAB><digest hex>. CC-BY-4.0."]
lines += [f"{n}\t{m.hex()}\t{d}" for n, m, d in EXAMPLES]
lines.append(f"million-a\trepeat:61:1000000\t{MILLION_A}")
write(HERE / "sha256-cavp-v1" / "fips180-examples.txt", lines)

rng = random.Random(1804)


def rsp(title, lengths):
    out = [f"# {title}", "# CAVP SHA256 byte-oriented layout; pseudo-random messages (seed 1804);",
           "# digests from Python hashlib (make_kci_fixtures.py). Not the NIST files. CC-BY-4.0.", "", "[L = 32]", ""]
    for n in lengths:
        msg = rng.randbytes(n)
        out += [f"Len = {8 * n}", f"Msg = {msg.hex() if n else '00'}", f"MD = {sha(msg)}", ""]
    return out


write(HERE / "sha256-cavp-v1" / "short-msg.rsp", rsp("SHA-256 ShortMsg (0..512 bits)", range(0, 65)))
write(HERE / "sha256-cavp-v1" / "long-msg.rsp", rsp("SHA-256 LongMsg", [163 + 99 * i for i in range(64)]))

# --- canonical JSON writer -----------------------------------------------------------------
# Kernel canonical JSON: strings, arrays and objects only (RFC 8785 rules; numbers never occur).
# Tree encoding (space-separated tokens): `{` `}` `[` `]`, `s<utf8 hex>` string value,
# `k<utf8 hex>` object key (always followed by its value).


def key16(k):
    return k.encode("utf-16-be")


def canon(v):
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    pairs = v[1]  # ("obj", [(k, v), ...])
    keys = [k for k, _ in pairs]
    if len(set(keys)) != len(keys):
        raise KeyError("duplicate")
    return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + canon(x)
                          for k, x in sorted(pairs, key=lambda p: key16(p[0]))) + "}"


def tokens(v):
    if isinstance(v, str):
        return ["s" + v.encode("utf-8").hex()]
    if isinstance(v, list):
        return ["["] + [t for x in v for t in tokens(x)] + ["]"]
    out = ["{"]
    for k, x in v[1]:
        out.append("k" + k.encode("utf-8").hex())
        out += tokens(x)
    return out + ["}"]


ALPHABET = (["a", "b", "z", "A", "0", " ", "/", "\"", "\\", "\x7f", "é", "€", " ",
             "", "￿", "\U0001f600", "\U00010000"] + [chr(c) for c in range(0x20)])


def rstr(r, n=6):
    return "".join(r.choice(ALPHABET) for _ in range(r.randint(0, n)))


def rtree(r, depth):
    t = r.random()
    if depth == 0 or t < 0.4:
        return rstr(r)
    if t < 0.7:
        return [rtree(r, depth - 1) for _ in range(r.randint(0, 4))]
    keys = []
    while len(keys) < r.randint(0, 5):
        k = rstr(r, 3)
        if k not in keys:
            keys.append(k)
    return ("obj", [(k, rtree(r, depth - 1)) for k in keys])


cases = [
    ("empty-string", ""),
    ("empty-array", []),
    ("empty-object", ("obj", [])),
    ("short-escapes", "\"\\\b\f\n\r\t"),
    ("u-escapes", "".join(chr(c) for c in range(0x20)) + "\x7f/"),
    ("non-ascii-raw", "é€ ￿\U0001f600"),
    ("sort-ascii", ("obj", [("b", "1"), ("a", "2"), ("aa", "3"), ("A", "4"), ("", "5")])),
    # UTF-16 order puts the surrogate pair of U+1F600 (D83D) before U+E000; code-point order would not.
    ("sort-utf16-vs-codepoint", ("obj", [("", "x"), ("\U0001f600", "y"), ("￿", "z"), ("é", "w")])),
    ("nested", ("obj", [("z", ["1", ("obj", [("y", []), ("x", "")])]), ("a", ("obj", []))])),
    ("dup-key", ("obj", [("a", "1"), ("b", "2"), ("a", "3")])),
    ("dup-key-nested", ["x", ("obj", [("k", ("obj", [("q", "1"), ("q", "1")]))])]),
]
r = random.Random(8785)
for i in range(60):
    cases.append((f"random-{i:02d}", rtree(r, 4)))
lines = ["# KRN.MAN.KCI canonical JSON writer cases. Format: <name><TAB><tokens><TAB><expected utf8 hex | ERR duplicate_key>.",
         "# Tokens: { } [ ] s<utf8 hex> (string value) k<utf8 hex> (object key). CC-BY-4.0."]
for name, v in cases:
    try:
        exp = canon(v).encode("utf-8").hex()
    except KeyError:
        exp = "ERR duplicate_key"
    lines.append(f"{name}\t{' '.join(tokens(v))}\t{exp}")
write(HERE / "canon-json-v1.txt", lines)

# --- kernel calls --------------------------------------------------------------------------
# test.echo.identity@1.0.0 (defined in crates/fid-kci/tests/kci.rs):
#   validate: params must be printable ASCII (0x20..0x7e), else invalid_params
#   run: params ++ "|" ++ (seed in decimal, or "-") ++ for each input: "|" ++ input
# KernelResult canonical JSON: {"algorithm","input_digests":[...],"output":<hex>,"output_digest",
#   "params_digest","seed":<decimal string, omitted when absent>}, digests as "sha256:<hex>".
ALG = "test.echo.identity@1.0.0"


def echo(params, seed, inputs):
    out = params + b"|" + (str(seed).encode() if seed is not None else b"-")
    for x in inputs:
        out += b"|" + x
    return out


def result_json(params, seed, inputs):
    out = echo(params, seed, inputs)
    obj = {"algorithm": ALG, "input_digests": ["sha256:" + sha(x) for x in inputs], "output": out.hex(),
           "output_digest": "sha256:" + sha(out), "params_digest": "sha256:" + sha(params)}
    if seed is not None:
        obj["seed"] = str(seed)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


r = random.Random(2026)
calls = [
    (b"", None, []),
    (b"{}", 0, []),
    (b"mode=a", 1, [b""]),
    (b"x", 2**64 - 1, [b"abc"]),
    (b"two inputs", 42, [b"abc", b"def"]),
    (b"three", None, [b"", b"", b""]),
    (b"p55", 7, [bytes(55)]),
    (b"p56", 8, [bytes(56)]),
    (b"p63", 9, [bytes(63)]),
    (b"p64", 10, [bytes(64)]),
    (b"p65", 11, [bytes(65)]),
    (b"~ !\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}", 12345678901234567890, [b"\x00\xff"]),
]
while len(calls) < 20:
    n = r.randint(0, 5)
    params = bytes(r.randint(0x20, 0x7e) for _ in range(r.randint(0, 40)))
    seed = r.choice([None, r.randrange(2**64)])
    calls.append((params, seed, [r.randbytes(r.randint(0, 300)) for _ in range(n)]))
lines = ["# KRN.MAN.KCI AC-2: 20 calls of test.echo.identity@1.0.0. Claimed digests are the true SHA-256.",
         "# Format: <name><TAB><params hex><TAB><seed | -><TAB><inputs: comma-separated hex | -><TAB><golden KernelResult JSON>.",
         "# CC-BY-4.0."]
for i, (p, s, ins) in enumerate(calls):
    ins_field = ",".join(x.hex() for x in ins) if ins else "-"
    lines.append(f"call-{i:02d}\t{p.hex()}\t{'-' if s is None else s}\t{ins_field}\t{result_json(p, s, ins)}")
assert len(calls) == 20
write(HERE / "kci-calls-v1.txt", lines)

# --- card canonical bytes (REQ-KRN.MAN.CARD-005) -------------------------------------------
# Canonical card bytes: canonical JSON of all card fields, keys sorted, lists kept in order.
lines = ["# KRN.MAN.CARD-005 card digests. Format: <card file><TAB><canonical JSON><TAB><sha256 hex>. CC-BY-4.0."]
for f in sorted((HERE / "cards-v1").glob("valid-*.toml")):
    card = tomllib.loads(f.read_text(encoding="utf-8"))
    text = json.dumps(card, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    lines.append(f"{f.name}\t{text}\t{sha(text.encode('utf-8'))}")
write(HERE / "card-digests-v1.txt", lines)
if STALE:
    for p in STALE:
        print(f"stale: {p.relative_to(HERE.parent.parent).as_posix()}", file=sys.stderr)
    sys.exit(1)
print("ok")
