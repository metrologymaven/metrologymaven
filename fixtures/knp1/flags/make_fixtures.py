#!/usr/bin/env python3
"""Generate fixtures/knp1/flags/ (spec node KRN.NUM.LINT.FLAGS, AC-1 .. AC-5).

Independent reference for the `flags` module of the Rust crate `fid-knp1-lint`.
Every config file is written literally below; the wasm modules are assembled
byte by byte here from the WebAssembly binary format (core spec section 5) and
the SIMD / relaxed-SIMD proposal opcode tables, with their text form written
next to them as `.wat`. Expected findings are hand-labelled from the spec's
rules: rule ID, workspace-relative file, 1-based line and 1-based column, where
the column is found here with str.find on the offending text (or is 1 for a
section header, a missing file, or a default), and for wasm the byte offset of
the instruction + 1. Nothing here runs the Rust code.

CC-BY-4.0 project data. Run:  python fixtures/knp1/flags/make_fixtures.py
Check mode (no writes, exit 1 on drift):  python fixtures/knp1/flags/make_fixtures.py --check
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CHECK = "--check" in sys.argv
DRIFT = []

TOOLCHAIN_OK = '[toolchain]\nchannel = "1.87.0"\ncomponents = ["rustfmt", "clippy"]\n'
CARGO_OK = """[workspace]
members = []
resolver = "2"

[profile.release]
codegen-units = 1

[profile.kernel]
inherits = "release"
"""


def put(rel: str, data) -> None:
    if isinstance(data, str):
        data = data.encode("utf-8")
    p = HERE / rel
    if CHECK:
        if not p.exists() or p.read_bytes() != data:
            DRIFT.append(rel)
        return
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)


def col(text: str, line: int, needle: str) -> int:
    """1-based column of the first `needle` on 1-based `line` of `text`."""
    row = text.split("\n")[line - 1]
    i = row.find(needle)
    assert i >= 0, (line, needle)
    return i + 1


def expect(rel: str, rows) -> None:
    """rows: (rule, file, line, column); written in report order (file, line, column, rule)."""
    rows = sorted(rows, key=lambda r: (r[1], r[2], r[3], r[0]))
    put(rel, "".join(f"{r} {f}:{l}:{c}\n" for r, f, l, c in rows))


# ---------------------------------------------------------------- wasm assembly

def uleb(n: int) -> bytes:
    out = bytearray()
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def vec(items) -> bytes:
    return uleb(len(items)) + b"".join(items)


def section(sid: int, content: bytes) -> bytes:
    return bytes([sid]) + uleb(len(content)) + content


def functype(params, results) -> bytes:
    return b"\x60" + vec([bytes([p]) for p in params]) + vec([bytes([r]) for r in results])


I32, I64, F64, V128 = 0x7F, 0x7E, 0x7C, 0x7B
SIMD = b"\xfd"


def simd(sub: int) -> bytes:
    return SIMD + uleb(sub)


def module(types, funcs, bodies, memory=False):
    """Returns (bytes, list of absolute offsets of each body's start)."""
    head = b"\x00asm" + b"\x01\x00\x00\x00"
    head += section(1, vec(types))
    head += section(3, vec([uleb(t) for t in funcs]))
    if memory:
        head += section(5, vec([b"\x00" + uleb(1)]))
    sized = [uleb(len(b)) + b for b in bodies]
    code_content = uleb(len(bodies)) + b"".join(sized)
    code = bytes([10]) + uleb(len(code_content))
    starts = []
    pos = len(head) + len(code) + len(uleb(len(bodies)))
    for b in bodies:
        pos += len(uleb(len(b)))
        starts.append(pos)
        pos += len(b)
    return head + code + code_content, starts


def local_get(i):
    return b"\x20" + uleb(i)


# relaxed.wasm: one f64x2.relaxed_madd (0xfd 0x107).
REL_PREFIX = b"\x00" + local_get(0) + local_get(1) + local_get(2)
relaxed_body = REL_PREFIX + simd(0x107) + b"\x0b"
relaxed, rel_starts = module([functype([V128, V128, V128], [V128])], [0], [relaxed_body])
RELAXED_OFFSET = rel_starts[0] + len(REL_PREFIX)
assert relaxed[RELAXED_OFFSET:RELAXED_OFFSET + 3] == b"\xfd\x87\x02"

# swizzle.wasm: one i8x16.relaxed_swizzle (0xfd 0x100), not a multiply-add.
SW_PREFIX = b"\x00" + local_get(0) + local_get(1)
swizzle, sw_starts = module([functype([V128, V128], [V128])], [0], [SW_PREFIX + simd(0x100) + b"\x0b"])
SWIZZLE_OFFSET = sw_starts[0] + len(SW_PREFIX)

# clean.wasm: fixed-width SIMD plus immediates that contain the madd byte pattern fd 87 02,
# so only an operator-by-operator walk gets it right.
TRAP = b"\xfd\x87\x02"
c0_prefix = b"\x00" + local_get(0) + local_get(1)
c0 = (
    c0_prefix
    + simd(0xF0)                                   # f64x2.add
    + simd(0x0C) + TRAP + bytes(13)                # v128.const with fd 87 02 inside
    + simd(0xF2)                                   # f64x2.mul
    + b"\x0b"
)
c1 = (
    vec([b"\x01" + bytes([F64])])                  # one f64 local
    + b"\x02\x40"                                  # block
    + local_get(0)
    + b"\x0e" + vec([uleb(0)]) + uleb(0)           # br_table [0] 0
    + b"\x0b"                                      # end
    + local_get(0) + b"\x28\x02\x00" + b"\x1a"     # i32.load align=2 offset=0; drop
    + b"\x42" + TRAP + b"\x1a"                     # i64.const (s64 bytes fd 87 02); drop
    + b"\x44" + bytes([0xFD, 0x87, 0x02, 0, 0, 0, 0xF0, 0x3F])  # f64.const
    + b"\x0b"
)
clean, clean_starts = module(
    [functype([V128, V128], [V128]), functype([I32], [F64])], [0, 1], [c0, c1], memory=True
)
CLEAN_ADD_OFFSET = clean_starts[0] + len(c0_prefix)
CLEAN_CONST_OFFSET = CLEAN_ADD_OFFSET + len(simd(0xF0))
CLEAN_MUL_OFFSET = CLEAN_CONST_OFFSET + 2 + 16
assert clean[CLEAN_MUL_OFFSET:CLEAN_MUL_OFFSET + 3] == b"\xfd\xf2\x01"

truncated = relaxed[:-3]

RELAXED_WAT = """(module
  (func (param v128 v128 v128) (result v128)
    local.get 0
    local.get 1
    local.get 2
    f64x2.relaxed_madd))
"""
SWIZZLE_WAT = """(module
  (func (param v128 v128) (result v128)
    local.get 0
    local.get 1
    i8x16.relaxed_swizzle))
"""
CLEAN_WAT = """(module
  (memory 1)
  (func (param v128 v128) (result v128)
    local.get 0
    local.get 1
    f64x2.add
    v128.const i8x16 0xfd 0x87 0x02 0 0 0 0 0 0 0 0 0 0 0 0 0
    f64x2.mul)
  (func (param i32) (result f64) (local f64)
    block
      local.get 0
      br_table 0 0
    end
    local.get 0
    i32.load
    drop
    i64.const 0x83fd  ;; LEB bytes fd 87 02
    drop
    f64.const 0x1.00000000287fdp+0  ;; bytes fd 87 02 00 00 00 f0 3f
  ))
"""

put("wasm/relaxed.wasm", relaxed)
put("wasm/relaxed.wat", RELAXED_WAT)
put("wasm/swizzle.wasm", swizzle)
put("wasm/swizzle.wat", SWIZZLE_WAT)
put("wasm/clean.wasm", clean)
put("wasm/clean.wat", CLEAN_WAT)
put("wasm/truncated.wasm", truncated)
put("ok/kernel.wasm", clean)

# Per-module expectations for scans with the default SIMD policy and with allowed_simd = [12]
# (v128.const only). Truncated: rule only (the stop offset is decoder detail).
put(
    "wasm/expected-scan.txt",
    "".join(
        [
            f"relaxed.wasm default KNP1-FLAG-WASMFMA 1:{RELAXED_OFFSET + 1}\n",
            f"swizzle.wasm default KNP1-FLAG-WASMSIMD 1:{SWIZZLE_OFFSET + 1}\n",
            f"clean.wasm only-12 KNP1-FLAG-WASMSIMD 1:{CLEAN_ADD_OFFSET + 1}\n",
            f"clean.wasm only-12 KNP1-FLAG-WASMSIMD 1:{CLEAN_MUL_OFFSET + 1}\n",
            f"relaxed.wasm only-12 KNP1-FLAG-WASMFMA 1:{RELAXED_OFFSET + 1}\n",
        ]
    ),
)

# ---------------------------------------------------------------- config directories

LINT_BASIC = '[flags]\nprofiles = ["release"]\n'

# AC-1: native
NATIVE_CFG = '[build]\nrustflags = ["-C", "target-cpu=native"]\n'
put("native/knp1-lint.toml", LINT_BASIC)
put("native/rust-toolchain.toml", TOOLCHAIN_OK)
put("native/Cargo.toml", CARGO_OK)
put("native/.cargo/config.toml", NATIVE_CFG)
expect("native/expected-findings.txt", [
    ("KNP1-FLAG-NATIVE", ".cargo/config.toml", 2, col(NATIVE_CFG, 2, "target-cpu=native")),
])

# AC-2: fma (three spellings: dotted target key, string rustflags, configured feature)
FMA_CFG = """# Seeded KNP1-FLAG-FMA (AC-2).
target.x86_64-unknown-linux-gnu.rustflags = ["-C", "target-feature=+avx2,+fma"]

[target.wasm32-unknown-unknown]
rustflags = "-C target-feature=+simd128,+relaxed-simd"

[host]
rustflags = ["-Ctarget-feature=-fma,+fma4"]
"""
put("fma/knp1-lint.toml", '[flags]\nprofiles = ["release"]\nfma_features = ["+fma4"]\n')
put("fma/rust-toolchain.toml", TOOLCHAIN_OK)
put("fma/Cargo.toml", CARGO_OK)
put("fma/.cargo/config.toml", FMA_CFG)
expect("fma/expected-findings.txt", [
    ("KNP1-FLAG-FMA", ".cargo/config.toml", 2, col(FMA_CFG, 2, "target-feature=+avx2,+fma")),
    ("KNP1-FLAG-FMA", ".cargo/config.toml", 5, col(FMA_CFG, 5, "target-feature=+simd128,+relaxed-simd")),
    ("KNP1-FLAG-FMA", ".cargo/config.toml", 8, col(FMA_CFG, 8, "target-feature=-fma,+fma4")),
])

# AC-3: codegen units. release is left at Cargo's default (16); kernel inherits release but
# sets 1; config.toml overrides one package and passes -C codegen-units=2; the CI file sets
# CARGO_PROFILE_KERNEL_CODEGEN_UNITS.
CGU_CARGO = """[workspace]
members = []
resolver = "2"

[profile.release]
opt-level = 3

[profile.kernel]
inherits = "release"
codegen-units = 1
"""
CGU_CFG = """[build]
rustflags = "-C codegen-units=2"

[profile.kernel.package.fid-num]
codegen-units = 4
"""
CGU_CI = "env:\n  CARGO_PROFILE_KERNEL_CODEGEN_UNITS: 8\n  CARGO_PROFILE_RELEASE_OPT_LEVEL: 3\n"
put("cgu/knp1-lint.toml", '[flags]\nprofiles = ["kernel", "release"]\nci_env_files = ["ci.yml"]\n')
put("cgu/rust-toolchain.toml", TOOLCHAIN_OK)
put("cgu/Cargo.toml", CGU_CARGO)
put("cgu/.cargo/config.toml", CGU_CFG)
put("cgu/ci.yml", CGU_CI)
expect("cgu/expected-findings.txt", [
    ("KNP1-FLAG-CGU", ".cargo/config.toml", 2, col(CGU_CFG, 2, "codegen-units=2")),
    ("KNP1-FLAG-CGU", ".cargo/config.toml", 5, 1),
    ("KNP1-FLAG-CGU", "Cargo.toml", 5, 1),
    ("KNP1-FLAG-CGU", "ci.yml", 2, col(CGU_CI, 2, "CARGO_PROFILE_KERNEL_CODEGEN_UNITS")),
])

# AC-3: toolchain channel "stable"
TC_STABLE = '[toolchain]\nchannel = "stable"\ncomponents = ["rustfmt"]\n'
put("toolchain/knp1-lint.toml", LINT_BASIC)
put("toolchain/rust-toolchain.toml", TC_STABLE)
put("toolchain/Cargo.toml", CARGO_OK)
expect("toolchain/expected-findings.txt", [("KNP1-FLAG-TOOLCHAIN", "rust-toolchain.toml", 2, 1)])

# REQ-004: missing rust-toolchain.toml
put("toolchain-missing/knp1-lint.toml", LINT_BASIC)
put("toolchain-missing/Cargo.toml", CARGO_OK)
expect("toolchain-missing/expected-findings.txt", [("KNP1-FLAG-TOOLCHAIN", "rust-toolchain.toml", 1, 1)])

# Scope: channel without patch level and no components pin
put("toolchain-nocomp/knp1-lint.toml", LINT_BASIC)
put("toolchain-nocomp/rust-toolchain.toml", '[toolchain]\nchannel = "1.87"\n')
put("toolchain-nocomp/Cargo.toml", CARGO_OK)
expect("toolchain-nocomp/expected-findings.txt", [
    ("KNP1-FLAG-TOOLCHAIN", "rust-toolchain.toml", 1, 1),
    ("KNP1-FLAG-TOOLCHAIN", "rust-toolchain.toml", 2, 1),
])

# KNP1-FLAG-LTO: kernel sets lto = "fat", release keeps the default; reported at release.
LTO_CARGO = CARGO_OK + 'lto = "fat"\n'
put("lto/knp1-lint.toml", '[flags]\nprofiles = ["kernel", "release"]\n')
put("lto/knp1-lint-allowed.toml", '[flags]\nprofiles = ["kernel", "release"]\nallowed_profile_differences = ["lto"]\n')
put("lto/rust-toolchain.toml", TOOLCHAIN_OK)
put("lto/Cargo.toml", LTO_CARGO)
expect("lto/expected-findings.txt", [("KNP1-FLAG-LTO", "Cargo.toml", 5, 1)])

# CI env file: RUSTFLAGS and CARGO_ENCODED_RUSTFLAGS (0x1f written as the YAML escape \x1f).
CI_YML = """name: kernel
# RUSTFLAGS: "-C target-cpu=native" is banned (comment line, ignored)
env:
  RUSTFLAGS: "-D warnings -C target-cpu=native"
  CARGO_ENCODED_RUSTFLAGS: "-C\\x1ftarget-feature=+relaxed-simd"
  KERNEL_FLAGS: ["-C", "target-cpu=native"]
  RUSTUP_TOOLCHAIN: stable
jobs:
  build:
    steps:
      - run: cargo build --profile release
      - run: RUSTFLAGS=-Ctarget-cpu=native cargo build
      - run: export RUSTFLAGS=-C target-cpu=native
      - run: export CARGO_ENCODED_RUSTFLAGS=-Ctarget-cpu=native
      - run: cargo +nightly build -Zbuild-std
      - run: cargo rustc -- -C llvm-args=-fp-contract=fast
      - run: cargo rustc -- -C llvm-args=-fp-contract=off
"""
put("ci/knp1-lint.toml", '[flags]\nprofiles = ["release"]\nci_env_files = ["workflows/kernel.yml"]\n')
put("ci/rust-toolchain.toml", TOOLCHAIN_OK)
put("ci/Cargo.toml", CARGO_OK)
put("ci/workflows/kernel.yml", CI_YML)
expect("ci/expected-findings.txt", [
    ("KNP1-FLAG-NATIVE", "workflows/kernel.yml", 4, col(CI_YML, 4, "target-cpu=native")),
    ("KNP1-FLAG-FMA", "workflows/kernel.yml", 5, col(CI_YML, 5, "target-feature=+relaxed-simd")),
    # array syntax: the comma between "-C" and its value must not hide it
    ("KNP1-FLAG-NATIVE", "workflows/kernel.yml", 6, col(CI_YML, 6, "target-cpu=native")),
    ("KNP1-FLAG-TOOLCHAIN", "workflows/kernel.yml", 7, col(CI_YML, 7, "RUSTUP_TOOLCHAIN")),
    # unquoted NAME=-Cflag, export NAME=-C flag, export NAME=-Cflag
    ("KNP1-FLAG-NATIVE", "workflows/kernel.yml", 12, col(CI_YML, 12, "target-cpu=native")),
    ("KNP1-FLAG-NATIVE", "workflows/kernel.yml", 13, col(CI_YML, 13, "target-cpu=native")),
    ("KNP1-FLAG-NATIVE", "workflows/kernel.yml", 14, col(CI_YML, 14, "target-cpu=native")),
    ("KNP1-FLAG-TOOLCHAIN", "workflows/kernel.yml", 15, col(CI_YML, 15, "cargo +nightly")),
    ("KNP1-FLAG-FMA", "workflows/kernel.yml", 16, col(CI_YML, 16, "llvm-args=-fp-contract=fast")),
])

# target-cpu values outside allowed_target_cpus (haswell implies FMA); the listed one passes.
CPU_CFG = """[build]
rustflags = ["-C", "target-cpu=x86-64-v2"]

[target.x86_64-unknown-linux-gnu]
rustflags = ["-C", "target-cpu=haswell"]
"""
put("cpu/knp1-lint.toml", '[flags]\nprofiles = ["release"]\nallowed_target_cpus = ["x86-64-v2"]\n')
put("cpu/rust-toolchain.toml", TOOLCHAIN_OK)
put("cpu/Cargo.toml", CARGO_OK)
put("cpu/.cargo/config.toml", CPU_CFG)
expect("cpu/expected-findings.txt", [
    ("KNP1-FLAG-CPU", ".cargo/config.toml", 5, col(CPU_CFG, 5, "target-cpu=haswell")),
])

# Cargo.toml [profile.*] rustflags, and a package override of codegen-units on the parent of a
# custom kernel profile (reached through `inherits`).
PF_CARGO = """[workspace]
members = []
resolver = "2"

[profile.release]
codegen-units = 1
rustflags = ["-C", "target-feature=+fma"]

[profile.kernel]
inherits = "release"

[profile.release.package.fid-num]
codegen-units = 8
"""
put("profile-flags/knp1-lint.toml", '[flags]\nprofiles = ["kernel"]\n')
put("profile-flags/rust-toolchain.toml", TOOLCHAIN_OK)
put("profile-flags/Cargo.toml", PF_CARGO)
expect("profile-flags/expected-findings.txt", [
    ("KNP1-FLAG-FMA", "Cargo.toml", 7, col(PF_CARGO, 7, "target-feature=+fma")),
    ("KNP1-FLAG-CGU", "Cargo.toml", 13, 1),
])

# A legacy rust-toolchain file next to a pinned rust-toolchain.toml.
put("toolchain-legacy/knp1-lint.toml", LINT_BASIC)
put("toolchain-legacy/rust-toolchain.toml", TOOLCHAIN_OK)
put("toolchain-legacy/rust-toolchain", "stable\n")
put("toolchain-legacy/Cargo.toml", CARGO_OK)
expect("toolchain-legacy/expected-findings.txt", [("KNP1-FLAG-TOOLCHAIN", "rust-toolchain", 1, 1)])

# AC-4 through a config: the wasm workspace is otherwise clean.
put("wasm/knp1-lint.toml", '[flags]\nprofiles = ["release"]\nwasm_module = "relaxed.wasm"\n')
put("wasm/knp1-lint-missing.toml", '[flags]\nprofiles = ["release"]\nwasm_module = "absent.wasm"\n')
put("wasm/rust-toolchain.toml", TOOLCHAIN_OK)
put("wasm/Cargo.toml", CARGO_OK)
expect("wasm/expected-findings.txt", [("KNP1-FLAG-WASMFMA", "relaxed.wasm", 1, RELAXED_OFFSET + 1)])

# AC-5: everything clean (a disabled -fma and harmless flags are not findings).
OK_CFG = """[build]
rustflags = ["-C", "debuginfo=0", "-C", "target-feature=-fma"]

[env]
RUST_LOG = "warn"
"""
OK_CI = 'env:\n  RUSTFLAGS: "-D warnings"\n  CARGO_PROFILE_RELEASE_CODEGEN_UNITS: "1"\n'
put("ok/knp1-lint.toml", """[flags]
profiles = ["kernel", "release"]
ci_env_files = ["ci.yml"]
wasm_module = "kernel.wasm"
""")
put("ok/rust-toolchain.toml", TOOLCHAIN_OK)
put("ok/Cargo.toml", CARGO_OK)
put("ok/.cargo/config.toml", OK_CFG)
put("ok/ci.yml", OK_CI)
expect("ok/expected-findings.txt", [])

if CHECK:
    if DRIFT:
        print("drift:", *DRIFT, sep="\n  ")
        sys.exit(1)
    print("fixtures/knp1/flags: up to date")
