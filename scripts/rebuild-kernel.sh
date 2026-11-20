#!/usr/bin/env bash
# Rebuilds the kernel WASM reference artifact reproducibly (spec node KRN.MAN.REPRO,
# REQ-KRN.MAN.REPRO-001 and -003). Anyone can run it to confirm a released kernel's hash.
#
# usage: scripts/rebuild-kernel.sh --out <dir> [--vendor <dir>] [--inject-timestamp-for-test]
#
# What you need:
#   - the pinned toolchain of kernel-build/rust-toolchain.toml with the wasm32-wasip1 target,
#     either through rustup (`rustup toolchain install 1.89.0 --profile minimal --target
#     wasm32-wasip1`) or as the toolchain's own bin/ directory first on PATH;
#   - a C linker for the host (only to build the sealing tool, fid-wasm-check);
#   - git, or SOURCE_DATE_EPOCH set to the commit time of the source you build;
#   - network access once, for `cargo vendor --locked`; or a vendored directory from an earlier
#     run (<out>/work/vendor), passed with --vendor, and then no network at all.
#
# What it does:
#   1. checks that `rustc -vV` (run from kernel-build/, so rustup picks the pin) reports the
#      pinned release, commit hash and LLVM version, and stops otherwise;
#   2. vendors every dependency of the workspace from Cargo.lock (`cargo vendor --locked`; the
#      lockfile's checksums pin the bytes) unless --vendor is given;
#   3. builds the artifact package with the kernel-artifact profile and the static flags of
#      kernel-build/.cargo/config.toml, `--locked --offline` from the vendored sources, with a
#      clean environment (`env -i`), a fresh CARGO_HOME, SOURCE_DATE_EPOCH from the commit time,
#      and every absolute path (source, CARGO_HOME, vendor, target directory) remapped to the
#      fixed prefixes of kernel-build/kernel-build.toml;
#   4. seals the linked module (fid-wasm-check seal, KRN.MAN.WASM) into the artifact;
#   5. writes <out>/<bin>.wasm (the artifact), <out>/<bin>.link.wasm, <out>/rustc-vV.txt,
#      <out>/build-info.txt and <out>/SHA256SUMS, and prints the SHA-256 values.
#
# Compare the SHA-256 of <out>/<bin>.wasm with the artifact hash in the kernel's release
# manifest. Cargo also reads .cargo/config.toml files in the directories above the checkout and
# nothing here can stop that; build from a path whose parents hold none.
#
# --inject-timestamp-for-test links the build time into the module (a wasm build_id section).
# It exists only so CI can show that a non-reproducible input is caught and its section named
# (AC-3); never use it for a real build.
set -euo pipefail

die() {
  echo "rebuild-kernel: $*" >&2
  exit 1
}

src=$(cd "$(dirname "$0")/.." && pwd -P)
kb="$src/kernel-build"
out=
vendor=
inject=0
while [ $# -gt 0 ]; do
  case "$1" in
    --out) [ $# -ge 2 ] || die "--out needs a value"; out=$2; shift 2 ;;
    --vendor) [ $# -ge 2 ] || die "--vendor needs a value"; vendor=$2; shift 2 ;;
    --inject-timestamp-for-test) inject=1; shift ;;
    *) die "unknown argument $1 (usage: scripts/rebuild-kernel.sh --out <dir> [--vendor <dir>])" ;;
  esac
done
[ -n "$out" ] || die "usage: scripts/rebuild-kernel.sh --out <dir> [--vendor <dir>]"

# One `key = "value"` line of a section ("" for the top level) of a settings file.
setting() {
  local v
  v=$(awk -v sec="$2" -v key="$3" '
    /^[[:space:]]*\[/ { cur = $0; gsub(/^[[:space:]]*\[|\][[:space:]]*$/, "", cur); next }
    cur == sec && $0 ~ ("^[[:space:]]*" key "[[:space:]]*=") {
      v = $0; sub(/^[^=]*=[[:space:]]*"/, "", v); sub(/"[[:space:]]*$/, "", v); print v; n++
    }
    END { if (n != 1) exit 1 }' "$1") || die "$1: [$2] $3 is missing or repeated"
  case "$v" in
    "" | *[!A-Za-z0-9._/-]*) die "$1: [$2] $3 has an unexpected value" ;;
  esac
  printf '%s' "$v"
}

channel=$(setting "$kb/rust-toolchain.toml" toolchain channel)
target=$(setting "$kb/.cargo/config.toml" build target)
commit_hash=$(setting "$kb/kernel-build.toml" toolchain commit_hash)
llvm_version=$(setting "$kb/kernel-build.toml" toolchain llvm_version)
pkg=$(setting "$kb/kernel-build.toml" artifact package)
bin=$(setting "$kb/kernel-build.toml" artifact bin)
r_ws=$(setting "$kb/kernel-build.toml" remap workspace)
r_ch=$(setting "$kb/kernel-build.toml" remap cargo_home)
r_v=$(setting "$kb/kernel-build.toml" remap vendor)
r_t=$(setting "$kb/kernel-build.toml" remap target_dir)

mkdir -p "$out"
out=$(cd "$out" && pwd -P)
[ -z "$(ls -A "$out")" ] || die "$out is not empty"
case "$out/" in "$src/"*) die "--out must lie outside the source tree" ;; esac
case "$src/" in "$out/"*) die "the source tree must not lie inside --out" ;; esac
work="$out/work"
mkdir -p "$work/cargo-home"
for p in "$src" "$out"; do
  case "$p" in *"'"* | *'"'* | *'\'*) die "path $p contains a quote or backslash" ;; esac
done

if [ -n "${SOURCE_DATE_EPOCH:-}" ]; then
  sde=$SOURCE_DATE_EPOCH
else
  sde=$(GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TERMINAL_PROMPT=0 \
    git -c core.fsmonitor=false -C "$src" log -1 --format=%ct 2>/dev/null) ||
    die "not a git checkout: set SOURCE_DATE_EPOCH to the commit time of the source"
fi
case "$sde" in "" | *[!0-9]*) die "SOURCE_DATE_EPOCH must be decimal seconds, got '$sde'" ;; esac

# Runs a command in directory $1 with a clean environment.
clean() {
  local dir=$1
  shift
  local extra=()
  if [ -n "${RUSTUP_HOME:-}" ]; then extra+=("RUSTUP_HOME=$RUSTUP_HOME"); fi
  (cd "$dir" && env -i PATH="$PATH" HOME="${HOME:-/nonexistent}" ${extra[@]+"${extra[@]}"} \
    CARGO_HOME="$work/cargo-home" SOURCE_DATE_EPOCH="$sde" TZ=UTC LC_ALL=C "$@")
}

# 1. The toolchain is the pinned one.
vv=$(clean "$kb" rustc -vV) || die "cannot run rustc from kernel-build/ (is toolchain $channel installed?)"
printf '%s\n' "$vv" >"$out/rustc-vV.txt"
field() { printf '%s\n' "$vv" | sed -n "s/^$1: //p"; }
[ "$(field release)" = "$channel" ] || die "toolchain drift: rustc release '$(field release)', pinned '$channel'"
[ "$(field commit-hash)" = "$commit_hash" ] ||
  die "toolchain drift: rustc commit-hash '$(field commit-hash)', pinned '$commit_hash'"
[ "$(field 'LLVM version')" = "$llvm_version" ] ||
  die "toolchain drift: rustc LLVM version '$(field 'LLVM version')', pinned '$llvm_version'"

# 2. Vendored dependencies.
if [ -n "$vendor" ]; then
  vendor=$(cd "$vendor" && pwd -P)
  case "$vendor" in *"'"* | *'"'* | *'\'*) die "path $vendor contains a quote or backslash" ;; esac
else
  vendor="$work/vendor"
  clean "$kb" cargo vendor --manifest-path "$src/Cargo.toml" --locked --versioned-dirs --quiet "$vendor" \
    >"$work/vendor-config.toml"
fi
sources=(--config "source.crates-io.replace-with='fid-vendored'" --config "source.fid-vendored.directory='$vendor'")

# 3. The artifact.
flags="'--remap-path-prefix=$src=$r_ws', '--remap-path-prefix=$work/cargo-home=$r_ch'"
flags="$flags, '--remap-path-prefix=$vendor=$r_v', '--remap-path-prefix=$work/target=$r_t'"
if [ "$inject" = 1 ]; then
  ts=$(date -u +%s%N)
  case "$ts" in *[!0-9]*) ts="$(date -u +%s)$$" ;; esac
  if [ $((${#ts} % 2)) = 1 ]; then ts="0$ts"; fi
  flags="$flags, '-C', 'link-arg=--build-id=0x$ts'"
  echo "rebuild-kernel: TEST BUILD: build time $ts injected; this artifact is not reproducible" >&2
fi
clean "$kb" cargo build --manifest-path "$src/Cargo.toml" --locked --offline -p "$pkg" --bin "$bin" \
  --profile kernel-artifact --target-dir "$work/target" "${sources[@]}" \
  --config "target.$target.rustflags=[$flags]"
cp "$work/target/$target/kernel-artifact/$bin.wasm" "$out/$bin.link.wasm"

# 4. Seal it. The sealing tool is a host program built from the workspace root (outside
# kernel-build/, so the kernel target and profile do not apply), with the same toolchain.
clean "$src" env RUSTUP_TOOLCHAIN="$channel" cargo build --manifest-path "$src/Cargo.toml" --locked --offline \
  -p fid-wasm-check --bin fid-wasm-check --release --target-dir "$work/tool-target" "${sources[@]}"
"$work/tool-target/release/fid-wasm-check" seal "$out/$bin.link.wasm" "$out/$bin.wasm"

# 5. Hashes.
if command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum "$1" | cut -d' ' -f1; }
else
  sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
fi
{
  echo "source_date_epoch $sde"
  echo "toolchain $channel $commit_hash llvm $llvm_version"
  echo "target $target profile kernel-artifact package $pkg bin $bin"
  echo "inject_timestamp_for_test $inject"
} >"$out/build-info.txt"
(cd "$out" && for f in "$bin.wasm" "$bin.link.wasm"; do echo "$(sha "$f")  $f"; done) >"$out/SHA256SUMS"
cat "$out/SHA256SUMS"
