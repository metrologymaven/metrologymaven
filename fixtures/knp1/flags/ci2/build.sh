#!/bin/sh
# Shell continuation: -C and its value on separate lines.
cargo rustc -- -C \
target-cpu=native
cargo rustc -- -C \
opt-level=3
