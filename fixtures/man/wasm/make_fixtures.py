#!/usr/bin/env python3
"""Generate the KRN.MAN.WASM fixtures (CC-BY-4.0 project data) independently of the Rust code.

Every expected value here comes from Python's standard library (hashlib, json) or is
hand-labelled from the rules pinned in spec/krn/man/wasm/NODE.md (Design), never from the
Rust implementation.

- crates/fid-kernel-wasm/wasm-runtime.toml   the pinned runtime settings, in canonical form
- fixtures/man/wasm/runtime-digest.txt       SHA-256 of those canonical bytes (config_digest)
- fixtures/man/wasm/modules/*.wasm           hand-encoded WebAssembly modules (valid and broken)
- fixtures/man/wasm/modules/labels.txt       expected parse and import-check outcome per module
- fixtures/man/wasm/test.allow               the allowlist the module labels are checked against
- fixtures/man/wasm/calls-v1.txt             canonical calls with golden stdout and exit codes
- fixtures/man/wasm/manifest/*.json          kernel manifests cross-checked against a module

Run:          python fixtures/man/wasm/make_fixtures.py
Check (CI):   python fixtures/man/wasm/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
ROOT = HERE.parent.parent.parent
OUT = {}  # path -> bytes


def put(path, data):
    assert path not in OUT, path
    OUT[path] = data


def sha(b):
    return hashlib.sha256(b).hexdigest()


# --- runtime settings (REQ-KRN.MAN.WASM-004) -------------------------------------------------
# Canonical form: these ten keys, sorted by byte value, one `key = value` per line, LF endings,
# no comments or blank lines. The file's SHA-256 is replay_runtime.config_digest.
RUNTIME = {
    "clock": '"trap"',  # clock imports are sealed to trap (fid_trap module, unknown imports trap)
    "fuel": "4000000000",  # wasmtime fuel per run; running out is NOT_REPLAYABLE(resource)
    "grow_failure": '"trap"',  # a failed memory.grow traps in the runtime
    "max_memory_size": "1073741824",  # 1 GiB of linear memory
    "max_wasm_stack": "1048576",  # 1 MiB of native wasm stack
    "nan_canonicalization": "true",
    "relaxed_simd": "false",
    "schema": '"1"',
    "threads": "false",
    "wasmtime_version": '"49.0.6"',  # the version the CI wasm32 job installs
}
runtime_bytes = "".join(f"{k} = {RUNTIME[k]}\n" for k in sorted(RUNTIME)).encode("ascii")
RUNTIME_DIGEST = "sha256:" + sha(runtime_bytes)
put(ROOT / "crates" / "fid-kernel-wasm" / "wasm-runtime.toml", runtime_bytes)
put(HERE / "runtime-digest.txt", (RUNTIME_DIGEST + "\n").encode("ascii"))


# --- WebAssembly binary encoding (WebAssembly Core Specification 2.0, section 5) --------------
def uleb(n, pad_to=0):
    """Unsigned LEB128; `pad_to` forces a longer, still valid, encoding."""
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n or len(out) + 1 < pad_to:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def vec(items):
    return uleb(len(items)) + b"".join(items)


def name(s):
    return uleb(len(s)) + s


def section(sid, payload, size=None):
    return bytes([sid]) + (uleb(len(payload)) if size is None else size) + payload


HEADER = b"\x00asm\x01\x00\x00\x00"
I32 = 0x7F


def functype(params, results):
    return b"\x60" + vec([bytes([p]) for p in params]) + vec([bytes([r]) for r in results])


TYPES = section(1, vec([functype([I32, I32, I32, I32], [I32]), functype([I32], [])]))
WASI = b"wasi_snapshot_preview1"


def imp_func(mod, field, typeidx):
    return name(mod) + name(field) + b"\x00" + uleb(typeidx)


def imports(*entries):
    return section(2, vec(list(entries)))


FD_WRITE = imp_func(WASI, b"fd_write", 0)
PROC_EXIT = imp_func(WASI, b"proc_exit", 1)
RANDOM_GET = imp_func(WASI, b"random_get", 0)
# An exported function `_start` with an empty body, so valid modules look like real commands.
FUNCS = section(3, vec([uleb(1)]))
MEMORY = section(5, vec([b"\x00" + uleb(1)]))
EXPORTS = section(7, vec([name(b"_start") + b"\x00" + uleb(2)]))
CODE = section(10, vec([name(b"\x00\x0b")]))  # body: size 2, no locals, `end`
CUSTOM = section(0, name(b"producers") + b"\x00")
OK_BODY = TYPES + imports(FD_WRITE, PROC_EXIT) + FUNCS + MEMORY + EXPORTS + CODE + CUSTOM


def sleb(n):
    """Signed LEB128 (i32.const and i64.const immediates)."""
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if (n == 0 and not b & 0x40) or (n == -1 and b & 0x40):
            out.append(b)
            return bytes(out)
        out.append(b | 0x80)


def command(types, imps, body, data=b""):
    """A WASI command: the given types and imports, one exported `_start` (last type), one page
    of exported memory and an optional active data segment at address 0."""
    nimp = len(imps)
    secs = section(1, vec(types)) + (imports(*imps) if imps else b"")
    secs += section(3, vec([uleb(len(types) - 1)])) + MEMORY
    secs += section(7, vec([name(b"memory") + b"\x02" + uleb(0), name(b"_start") + b"\x00" + uleb(nimp)]))
    secs += section(10, vec([name(b"\x00" + body + b"\x0b")]))
    if data:
        secs += section(11, vec([b"\x00\x41" + sleb(0) + b"\x0b" + name(data)]))
    return HEADER + secs


CLOCK_MSG = b'{"error":"clock_read"}'
CLOCK_CALL = command(
    [functype([I32, 0x7E, I32], [I32]), functype([I32, I32, I32, I32], [I32]), functype([I32], []), functype([], [])],
    [imp_func(WASI, b"clock_time_get", 0), imp_func(WASI, b"fd_write", 1), imp_func(WASI, b"proc_exit", 2)],
    # clock_time_get(0, 0, 64); drop; fd_write(1, iov=0, 1, nwritten=72); drop; proc_exit(1)
    b"\x41" + sleb(0) + b"\x42" + sleb(0) + b"\x41" + sleb(64) + b"\x10\x00\x1a"
    + b"\x41" + sleb(1) + b"\x41" + sleb(0) + b"\x41" + sleb(1) + b"\x41" + sleb(72) + b"\x10\x01\x1a"
    + b"\x41" + sleb(1) + b"\x10\x02",
    # iovec {ptr 16, len} at 0, the message at 16
    (16).to_bytes(4, "little") + len(CLOCK_MSG).to_bytes(4, "little") + bytes(8) + CLOCK_MSG,
)
LOOP = command([functype([], [])], [], b"\x03\x40\x0c\x00\x0b")  # loop br 0 end


def seal(module):
    """Independent seal: rename the module of WASI clock function imports to `fid_trap`,
    re-encoding the import section with minimal LEB128 lengths and copying the rest."""
    out, i = bytearray(module[:8]), 8

    def rd(i):
        v = s = 0
        while True:
            b = module[i]
            i += 1
            v |= (b & 0x7F) << s
            s += 7
            if b < 0x80:
                return v, i

    while i < len(module):
        sid = module[i]
        size, j = rd(i + 1)
        end = j + size
        if sid != 2:
            out += module[i:end]
        else:
            n, k = rd(j)
            entries = []
            for _ in range(n):
                l, k = rd(k); mod = module[k:k + l]; k += l
                l, k = rd(k); fld = module[k:k + l]; k += l
                kind = module[k]
                assert kind == 0, "only function imports in these fixtures"
                _, d_end = rd(k + 1)
                if mod == WASI and fld in (b"clock_time_get", b"clock_res_get"):
                    mod = b"fid_trap"
                entries.append(name(mod) + name(fld) + module[k:d_end])
                k = d_end
            assert k == end
            out += section(2, vec(entries))
        i = end
    return bytes(out)


assert seal(seal(CLOCK_CALL)) == seal(CLOCK_CALL) != CLOCK_CALL
assert seal(HEADER + OK_BODY) == HEADER + OK_BODY

# (file, bytes, parse label, check label against test.allow)
# parse label: ok:<number of imports> | err:<code>; check label: pass | fail:<module name>,... | -
MODULES = [
    ("ok-empty.wasm", HEADER, "ok:0", "pass"),
    ("ok-wasi.wasm", HEADER + OK_BODY, "ok:2", "pass"),
    # Non-minimal LEB128 sizes are valid WebAssembly and are accepted.
    ("ok-padded-leb.wasm",
     HEADER + section(1, vec([functype([], [])]), size=uleb(4, pad_to=5)), "ok:0", "pass"),
    ("bad-random-get.wasm",
     HEADER + TYPES + imports(FD_WRITE, RANDOM_GET), "ok:2", "fail:wasi_snapshot_preview1 random_get"),
    # Only function imports can be allowlisted; a memory import is denied even with a known name.
    ("bad-memory-import.wasm",
     HEADER + imports(name(WASI) + name(b"fd_write") + b"\x02\x00" + uleb(1)), "ok:1",
     "fail:wasi_snapshot_preview1 fd_write"),
    ("bad-env-import.wasm",
     HEADER + TYPES + imports(FD_WRITE, imp_func(b"env", b"fd_write", 0)), "ok:2", "fail:env fd_write"),
    ("err-magic.wasm", b"\x00asn\x01\x00\x00\x00", "err:bad_magic", "-"),
    ("err-version-component.wasm", b"\x00asm\x0d\x00\x01\x00", "err:bad_version", "-"),
    ("err-truncated-header.wasm", b"\x00asm\x01\x00", "err:truncated", "-"),
    ("err-section-overrun.wasm", HEADER + b"\x01" + uleb(100) + b"\x01\x60\x00", "err:section_overrun", "-"),
    # Six LEB bytes for a u32.
    ("err-leb-long.wasm", HEADER + b"\x00\x80\x80\x80\x80\x80\x00", "err:bad_leb", "-"),
    # Five bytes whose last byte sets bits above 32.
    ("err-leb-overflow.wasm", HEADER + b"\x00\xff\xff\xff\xff\x1f", "err:bad_leb", "-"),
    ("err-duplicate-import.wasm",
     HEADER + TYPES + imports(FD_WRITE) + imports(PROC_EXIT), "err:section_order", "-"),
    ("err-order.wasm", HEADER + imports() + TYPES, "err:section_order", "-"),
    ("err-unknown-section.wasm", HEADER + section(14, b""), "err:unknown_section", "-"),
    ("err-utf8-name.wasm", HEADER + TYPES + imports(imp_func(b"\xff", b"fd_write", 0)), "err:bad_utf8", "-"),
    ("err-import-kind.wasm",
     HEADER + TYPES + imports(name(WASI) + name(b"fd_write") + b"\x05\x00"), "err:bad_import_kind", "-"),
    ("err-import-trailing.wasm",
     HEADER + TYPES + section(2, vec([FD_WRITE]) + b"\x00"), "err:section_size_mismatch", "-"),
    ("err-import-short.wasm", HEADER + TYPES + section(2, uleb(2) + FD_WRITE), "err:truncated", "-"),
    ("err-type-index.wasm", HEADER + TYPES + imports(imp_func(WASI, b"fd_write", 5)), "err:bad_type_index", "-"),
    ("err-no-types.wasm", HEADER + imports(FD_WRITE), "err:bad_type_index", "-"),
    ("err-custom-name.wasm", HEADER + section(0, uleb(9) + b"abc"), "err:truncated", "-"),
    ("err-trailing-byte.wasm", HEADER + TYPES + b"\x01", "err:truncated", "-"),
    ("err-valtype.wasm", HEADER + section(1, vec([b"\x60\x01\x40\x00"])), "err:bad_type", "-"),
    ("err-type-form.wasm", HEADER + section(1, vec([b"\x5f\x00\x00"])), "err:bad_type", "-"),
    ("err-shared-memory.wasm",
     HEADER + imports(name(b"env") + name(b"memory") + b"\x02\x03\x01\x01"), "err:bad_limits", "-"),
    ("err-limits-max.wasm",
     HEADER + imports(name(b"env") + name(b"memory") + b"\x02\x01\x05\x01"), "err:bad_limits", "-"),
    ("err-too-many-imports.wasm", HEADER + TYPES + section(2, uleb(4097)), "err:limit_exceeded", "-"),
    # A command that reads the clock and, only if that returns, writes {"error":"clock_read"} and
    # exits 1. Unsealed, the import check denies it; sealed, the clock call must trap.
    ("clock-call.wasm", CLOCK_CALL, "ok:3", "fail:wasi_snapshot_preview1 clock_time_get"),
    ("clock-call.sealed.wasm", seal(CLOCK_CALL), "ok:3", "pass"),
    # A command that loops forever: the runner's fuel limit must end it as a resource limit.
    ("loop.wasm", LOOP, "ok:0", "pass"),
]
labels = ["# KRN.MAN.WASM module labels (hand-labelled from the Design rules). CC-BY-4.0.",
          "# Format: <file><TAB><ok:<imports> | err:<code>><TAB><check against ../test.allow: pass | fail:<module name>,... | ->."]
for f, data, parse, check in MODULES:
    put(HERE / "modules" / f, data)
    labels.append(f"{f}\t{parse}\t{check}")
put(HERE / "modules" / "labels.txt", ("\n".join(labels) + "\n").encode("ascii"))

put(HERE / "test.allow", (
    "# Test allowlist for fixtures/man/wasm/modules (format of crates/fid-kernel-wasm/wasm-imports.allow).\n"
    "\n"
    "wasi_snapshot_preview1 fd_write -- test entry: writes bytes to a file descriptor\n"
    "wasi_snapshot_preview1 proc_exit -- test entry: ends the process with an exit code\n"
).encode("ascii"))


# --- canonical calls and golden outputs (AC-2, AC-3) -----------------------------------------
def call_json(alg, params, seed, inputs, claimed=None):
    claimed = claimed or [sha(b) for b in inputs]
    obj = {"algorithm": alg, "inputs": [{"bytes": b.hex(), "sha256": "sha256:" + d} for b, d in zip(inputs, claimed)],
           "params": params.hex()}
    if seed is not None:
        obj["seed"] = str(seed)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


def result_json(alg, params, seed, inputs, output):
    obj = {"algorithm": alg, "input_digests": ["sha256:" + sha(b) for b in inputs], "output": output.hex(),
           "output_digest": "sha256:" + sha(output), "params_digest": "sha256:" + sha(params)}
    if seed is not None:
        obj["seed"] = str(seed)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"))


def err(code):
    return '{"error":"%s"}' % code


ECHO = "test.echo.identity@1.0.0"
RECURSE = "test.stack.recurse@1.0.0"
ALLOC = "test.memory.alloc@1.0.0"
RESOURCE = "NOT_REPLAYABLE(resource)"

cases = []  # (name, mode, call, exit, stdout)
# The 20 calls of fixtures/man/kci-calls-v1.txt (KRN.MAN.KCI), golden results copied unchanged.
for line in (HERE.parent / "kci-calls-v1.txt").read_text(encoding="utf-8").splitlines():
    if not line or line.startswith("#"):
        continue
    name_, p, s, i, golden = line.split("\t")
    params = bytes.fromhex(p)
    seed = None if s == "-" else int(s)
    inputs = [] if i == "-" else [bytes.fromhex(x) for x in i.split(",")]
    # The echo rule of KRN.MAN.KCI: params | seed-or-"-" | input0 | input1 ...
    echo = b"|".join([params, b"-" if seed is None else str(seed).encode()] + inputs)
    assert golden == result_json(ECHO, params, seed, inputs, echo), name_
    cases.append((f"kci-{name_}", "any", call_json(ECHO, params, seed, inputs), 0, golden))
assert len(cases) == 20

# Kernel errors: exit 1, `{"error":"<KernelError code>"}`.
cases.append(("echo-digest-mismatch", "any", call_json(ECHO, b"x", None, [b"abc"], [sha(b"abd")]), 1,
              err("digest_mismatch")))
cases.append(("echo-asked-to-fail", "any", call_json(ECHO, b"fail", 7, []), 1, err("algorithm_failed")))
cases.append(("echo-nonprintable", "any", call_json(ECHO, b"\x01", None, []), 1, err("invalid_params")))
cases.append(("unknown-algorithm", "any", call_json("test.echo.identity@9.0.0", b"", None, []), 1,
              err("unknown_algorithm")))
cases.append(("recurse-bad-params", "any", call_json(RECURSE, b"01", None, []), 1, err("invalid_params")))
# KRN.MAN.KCI REQ-008: the recursion card declares input_arity "0", so one input is refused.
cases.append(("recurse-input-arity", "any", call_json(RECURSE, b"1", None, [b"x"]), 1, err("input_arity_mismatch")))
# Malformed calls: exit 2.
good = call_json(ECHO, b"ab", 3, [b"c"])
malformed = [
    ("malformed-space", good.replace(",", ", ", 1)),
    ("malformed-key-order", '{"inputs":[],"algorithm":"%s","params":""}' % ECHO),
    ("malformed-upper-hex", call_json(ECHO, b"\xab", None, []).replace('"ab"', '"AB"')),
    ("malformed-odd-hex", call_json(ECHO, b"", None, []).replace('"params":""', '"params":"a"')),
    ("malformed-seed-leading-zero", call_json(ECHO, b"", None, []).replace("}", ',"seed":"01"}')),
    ("malformed-seed-overflow", call_json(ECHO, b"", None, []).replace("}", ',"seed":"18446744073709551616"}')),
    ("malformed-seed-number", call_json(ECHO, b"", None, []).replace("}", ',"seed":5}')),
    ("malformed-bad-id", call_json("Test.echo.identity@1.0.0", b"", None, [])),
    ("malformed-id-leading-zero", call_json("test.echo.identity@01.0.0", b"", None, [])),
    ("malformed-digest-upper", call_json(ECHO, b"", None, [b"a"], [sha(b"a").upper()])),
    ("malformed-digest-short", call_json(ECHO, b"", None, [b"a"], [sha(b"a")[:-1]])),
    ("malformed-unknown-member", call_json(ECHO, b"", None, []).replace("}", ',"zz":""}')),
    ("malformed-trailing-data", good + " "),
    ("malformed-empty", ""),
]
for n, c in malformed:
    cases.append((n, "any", c, 2, err("malformed_call")))
# Stand-in resource algorithms: small sizes succeed; past the pinned limits only the WASM runner
# can observe the outcome (mode `wasm`), which is NOT_REPLAYABLE(resource), runner exit 3.
cases.append(("recurse-small", "any", call_json(RECURSE, b"100", None, []), 0,
              result_json(RECURSE, b"100", None, [], b"100")))
cases.append(("recurse-past-stack", "wasm", call_json(RECURSE, b"100000000", None, []), 3, RESOURCE))
cases.append(("alloc-small", "any", call_json(ALLOC, b"4096", 1, []), 0,
              result_json(ALLOC, b"4096", 1, [], b"4096")))
cases.append(("alloc-past-memory", "wasm", call_json(ALLOC, b"1610612736", None, []), 3, RESOURCE))

calls = ["# KRN.MAN.WASM calls through the shim (AC-2, AC-3). Written by make_fixtures.py. CC-BY-4.0.",
         "# Format: <name><TAB><any | wasm><TAB><canonical call><TAB><runner exit><TAB><expected stdout>.",
         "# `wasm` rows need the pinned WASM runtime limits and run only under fid-wasm-check."]
names = set()
for n, mode, c, code, out in cases:
    assert n not in names and "\t" not in c and "\n" not in c
    names.add(n)
    calls.append(f"{n}\t{mode}\t{c}\t{code}\t{out}")
put(HERE / "calls-v1.txt", ("\n".join(calls) + "\n").encode("ascii"))


# --- manifest cross-check (WASM artifact digest and replay runtime) --------------------------
def canon(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


BASE = json.loads((HERE.parent / "manifest-v1.json").read_text(encoding="utf-8"))
MODULE_DIGEST = "sha256:" + sha(HEADER + OK_BODY)


def manifest(wasm_target="wasm32-wasip1", wasm_digest=MODULE_DIGEST, config=RUNTIME_DIGEST, version="49.0.6"):
    core = json.loads(json.dumps(BASE["core"]))
    core["replay_runtime"] = {"config_digest": config, "version": version}
    arts = [a for a in BASE["artifacts"] if a["role"] != "wasm"]
    arts.append({"role": "wasm", "sha256": wasm_digest, "target": wasm_target})
    arts.sort(key=lambda a: (a["role"].encode(), a["target"].encode()))
    return canon({"artifacts": arts, "core": core, "core_digest": "sha256:" + sha(canon(core))})


other = "sha256:" + sha(b"other")
MANIFESTS = [
    ("ok.json", manifest(), "ok"),
    ("artifact-missing.json", manifest(wasm_target="wasm32-unknown-unknown"), "artifact_missing"),
    ("artifact-digest.json", manifest(wasm_digest=other), "artifact_digest_mismatch"),
    ("config-digest.json", manifest(config=other), "config_digest_mismatch"),
    ("runtime-version.json", manifest(version="49.0.7"), "runtime_version_mismatch"),
    # Strict reading (parse_canonical): the same manifest with a trailing newline is refused.
    ("non-canonical.json", manifest() + b"\n", "manifest:non_canonical"),
]
mlabels = ["# KRN.MAN.WASM manifest cross-check labels, against modules/ok-wasi.wasm, ../test.allow and",
           "# crates/fid-kernel-wasm/wasm-runtime.toml. Format: <file><TAB><ok | error code>. CC-BY-4.0."]
for f, data, label in MANIFESTS:
    put(HERE / "manifest" / f, data)
    mlabels.append(f"{f}\t{label}")
put(HERE / "manifest" / "labels.txt", ("\n".join(mlabels) + "\n").encode("ascii"))


def main():
    check = sys.argv[1:] == ["--check"]
    bad = 0
    for path, data in sorted(OUT.items()):
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path.relative_to(ROOT)}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        print(f"{len(OUT)} files checked, {bad} stale")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
