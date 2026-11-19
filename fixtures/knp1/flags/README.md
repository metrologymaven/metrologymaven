# fixtures/knp1/flags

Fixtures for spec node KRN.NUM.LINT.FLAGS (crate `fid-knp1-lint`, module `flags`). Every file is
written by `make_fixtures.py` (Python stdlib only), the independent reference: config files are
literal text, the wasm modules are assembled byte by byte from the WebAssembly binary format and
the SIMD / relaxed-SIMD opcode tables (text form in the `.wat` files next to them), and the expected
findings (`RULE file:line:column`) are hand-labelled from the spec's rules. Nothing here was
produced by running the Rust code. `python make_fixtures.py --check` reports drift.

Each directory is a small workspace (`knp1-lint.toml`, `rust-toolchain.toml`, `Cargo.toml`,
optional `.cargo/config.toml` and CI env files):

- `native/`: `build.rustflags = ["-C", "target-cpu=native"]`. AC-1, `KNP1-FLAG-NATIVE`.
- `fma/`: `target.x86_64-unknown-linux-gnu.rustflags` with `+avx2,+fma`, a string `rustflags` with
  `+relaxed-simd`, and `host.rustflags` with the configured feature `+fma4` (its `-fma` is a
  disable and does not count). AC-2, `KNP1-FLAG-FMA`.
- `cgu/`: `release` left at Cargo's default of 16 codegen units, a package override of 4, `-C
  codegen-units=2` in rustflags, and `CARGO_PROFILE_KERNEL_CODEGEN_UNITS: 8` in a CI file. AC-3,
  `KNP1-FLAG-CGU`.
- `toolchain/`: `channel = "stable"`. AC-3, `KNP1-FLAG-TOOLCHAIN`.
- `toolchain-missing/`: no `rust-toolchain.toml`. REQ-004.
- `toolchain-nocomp/`: `channel = "1.87"` and no `components`. Scope rule.
- `lto/`: kernel profile sets `lto = "fat"`, `release` keeps the default; `knp1-lint-allowed.toml`
  allows the difference. `KNP1-FLAG-LTO`.
- `ci/`: `RUSTFLAGS` and `CARGO_ENCODED_RUSTFLAGS` (with the `\x1f` escape) in a workflow file; a
  comment line naming `target-cpu=native` is ignored. Also array syntax `["-C", "target-cpu=native"]`,
  unquoted `RUSTFLAGS=-Ctarget-cpu=native`, `export RUSTFLAGS=-C target-cpu=native`,
  `export CARGO_ENCODED_RUSTFLAGS=-Ctarget-cpu=native`, `RUSTUP_TOOLCHAIN` and `cargo +nightly`
  (`KNP1-FLAG-TOOLCHAIN`), and `-C llvm-args=-fp-contract=fast` (`KNP1-FLAG-FMA`; `=off` passes).
- `cpu/`: `target-cpu=haswell` outside `allowed_target_cpus = ["x86-64-v2"]` (`KNP1-FLAG-CPU`);
  the allowed `x86-64-v2` passes.
- `profile-flags/`: `[profile.release] rustflags` with `+fma` in `Cargo.toml`, and a
  `[profile.release.package.fid-num] codegen-units = 8` reached from kernel profile `kernel`
  through `inherits`.
- `toolchain-legacy/`: a legacy `rust-toolchain` file next to a pinned `rust-toolchain.toml`.
- `wasm/`: `relaxed.wasm` (one `f64x2.relaxed_madd`), `clean.wasm` (fixed-width SIMD, with the
  madd byte pattern `fd 87 02` hidden inside `v128.const`, `i64.const` and `f64.const`
  immediates), `swizzle.wasm` (one `i8x16.relaxed_swizzle`, not a multiply-add), `truncated.wasm`
  (`relaxed.wasm` minus its last three bytes). `expected-scan.txt` lists the scan results with the
  default SIMD policy and with `allowed_simd = [12]`. AC-4.
- `ok/`: pinned toolchain, `codegen-units = 1`, no FMA flags, a clean CI file and `clean.wasm` as
  `kernel.wasm`. AC-5, no findings.

CC-BY-4.0 project data.
