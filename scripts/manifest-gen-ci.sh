#!/usr/bin/env bash
# KRN.MAN.MANI.GEN in CI (run from the repository root by the wasm32 job of rust.yml, which has
# already installed the wasm32-wasip1 target and the pinned wasmtime).
#
#   AC-1  both stages on fixtures/man/gen-toy-v1 give expected-core.json, expected-constant.rs
#         and expected-manifest.json (written by the fixture's Python script)
#   AC-2  every self-report goes through `fid-xart run` and the pinned runner
#         `fid-wasm-check run`, which passes wasmtime no --dir: no preopened directory
#   AC-3  an artifact built from a different core fails with core_digest_mismatch
#   AC-4  both stages run twice in different directories give identical bytes, stage one again
#         after the constant is embedded gives identical bytes, and a missing RDL release file
#         is missing_source: reference_suite_digest
#
# Then the real kernel module (fid-kernel-wasm, wasm32-wasip1 and native) is built with the
# generated constant and must report the stage-one core digest; built with the committed `None`
# constant it must fail. The committed constant is restored on exit.
set -euo pipefail

root=$(pwd)
cargo build -q --locked -p fid-manifest-gen -p fid-xart -p fid-wasm-check
export PATH="$root/target/debug:$PATH"
toy="$root/fixtures/man/gen-toy-v1"
work="${RUNNER_TEMP:-/tmp}/manifest-gen"
rm -rf "$work"
mkdir -p "$work"

expect_fail() { # <expected stdout> <command...>: exit 1 with exactly that line on stdout
  local want=$1 out code
  shift
  set +e
  out=$("$@")
  code=$?
  set -e
  echo "$out"
  test "$code" = 1 && test "$out" = "$want"
}

# AC-1, AC-2, AC-4: two runs in different directories.
for d in "$work/run-a/toy" "$work/run-b/nested/dir/toy"; do
  mkdir -p "$(dirname "$d")"
  cp -R "$toy" "$d"
  (
    cd "$d"
    fid-manifest-gen core --config manifest-gen.toml --out core.json --constant ws/shim/src/manifest_core_digest.rs > core-digest.txt
    cmp core.json "$toy/expected-core.json"
    cmp core-digest.txt "$toy/expected-core-digest.txt"
    cmp ws/shim/src/manifest_core_digest.rs "$toy/expected-constant.rs"
    # The embedded constant is outside the core: stage one again gives the same bytes.
    fid-manifest-gen core --config manifest-gen.toml --out core-again.json > /dev/null
    cmp core.json core-again.json
    fid-manifest-gen finish --core core.json --artifacts artifacts.toml --out kernel-manifest.json > manifest-digest.txt
    cmp kernel-manifest.json "$toy/expected-manifest.json"
    cmp manifest-digest.txt "$toy/expected-manifest-digest.txt"
    # The WASM cross-check of KRN.MAN.WASM agrees with the manifest (artifact hash, runtime).
    fid-wasm-check manifest kernel-manifest.json artifacts/toy-kernel.wasm wasm-imports.allow wasm-runtime.toml
    # AC-3: a module built from another core; no manifest is written.
    expect_fail "core_digest_mismatch: wasm-wasm32-wasip1" \
      fid-manifest-gen finish --core core.json --artifacts artifacts-other-core.toml --out other.json
    test ! -e other.json
  )
done
for f in core.json core-again.json kernel-manifest.json manifest-digest.txt ws/shim/src/manifest_core_digest.rs; do
  cmp "$work/run-a/toy/$f" "$work/run-b/nested/dir/toy/$f"
done

# AC-4 (error path): no RDL release file.
cp -R "$toy" "$work/no-rdl"
rm "$work/no-rdl/rdl-release.json"
(cd "$work/no-rdl" && expect_fail "missing_source: reference_suite_digest" \
  fid-manifest-gen core --config manifest-gen.toml --out core.json)
test ! -e "$work/no-rdl/core.json"

# The real kernel module with the generated constant (WASM through the pinned runner, and the
# native build of the same shim). There is no real release configuration yet (KRN.SOLV.CONIC
# publishes no limits file and UNC.RDL.FREEZE is unbuilt), so the module is built with the TOY
# core's constant: this checks the embedding path end to end (constant compiled in, self-report
# through fid-xart, finish), not a release. The resulting manifest pairs the toy core with these
# artifacts; it stays in RUNNER_TEMP, is never uploaded or published, and is deleted with the
# runner.
constant=crates/fid-kernel-wasm/src/manifest_core_digest.rs
trap 'git checkout -- "$constant"' EXIT
cp "$work/run-a/toy/ws/shim/src/manifest_core_digest.rs" "$constant"
cargo build -q --locked --release --target wasm32-wasip1 -p fid-kernel-wasm --target-dir target/manifest-gen-wasm
cargo build -q --locked -p fid-kernel-wasm --target-dir target/manifest-gen-native
fid-wasm-check seal target/manifest-gen-wasm/wasm32-wasip1/release/fid-kernel-wasm.wasm "$work/kernel.wasm"
cp target/manifest-gen-native/debug/fid-kernel-wasm "$work/kernel-native"
runtime="$root/crates/fid-kernel-wasm/wasm-runtime.toml"
allow="$root/crates/fid-kernel-wasm/wasm-imports.allow"
cat > "$work/kernel-artifacts.toml" <<EOF
runtime = "$runtime"
allow = "$allow"

[[artifact]]
role = "native"
target = "x86_64-unknown-linux-gnu"
file = "$work/kernel-native"

[[artifact]]
role = "wasm"
target = "wasm32-wasip1"
file = "$work/kernel.wasm"
EOF
fid-manifest-gen finish --core "$work/run-a/toy/core.json" --artifacts "$work/kernel-artifacts.toml" \
  --out "$work/kernel-manifest.json"
fid-wasm-check manifest "$work/kernel-manifest.json" "$work/kernel.wasm" "$allow" "$runtime"

# Built with the committed `None` constant, the module reports no core digest.
git checkout -- "$constant"
cargo build -q --locked -p fid-kernel-wasm --target-dir target/manifest-gen-native
cp target/manifest-gen-native/debug/fid-kernel-wasm "$work/kernel-native"
expect_fail "core_digest_mismatch: native-x86_64-unknown-linux-gnu" \
  fid-manifest-gen finish --core "$work/run-a/toy/core.json" --artifacts "$work/kernel-artifacts.toml" \
  --out "$work/unset.json"
echo "KRN.MAN.MANI.GEN: both stages identical in two directories; self-reports checked"
