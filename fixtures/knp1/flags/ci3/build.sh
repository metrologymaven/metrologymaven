#!/bin/sh
set -eu
RUSTFLAGS := -C target-cpu=native
RUSTFLAGS ?= -Ctarget-cpu=native
RUSTFLAGS += -Ctarget-feature=+fma
export RUSTFLAGS="${RUSTFLAGS:--C target-cpu=native}"
F="${KFLAGS:--Ctarget-cpu=native}"
export RUSTFLAGS="$(printf '\x2dC\x20target\x2dcpu=native')"
export CARGO_ENCODED_RUSTFLAGS="$(printf '%s\037%s' -C target-cpu=native)"
export RUSTFLAGS=$'-C\x20target-cpu=native'
X=$'\x2dCtarget-cpu=native'
cargo build --config   profile.release.codegen-units=16
cargo build --config=profile.release.codegen-units=16
cargo build --config 'profile.release.codegen-units = 16'
cargo build --config "profile.release = { codegen-units = 16 }"
cargo build --config "build = { rustflags = ['-C', 'target-cpu=native'] }"
cargo build --config ci/kernel-config.toml
cargo build --config "$KCFG"
rustc +nightly -O src/lib.rs
rustup run nightly cargo build
rustup override set nightly
rustup default nightly
cargo.exe +nightly build
cargo build --config net.retry=2
cargo build --release
