## House rules (Rust)

These rules hold in every repository that contains Rust code.

### Denied lints

The workspace denies `unwrap`, `expect`, `panic`, `todo`, `unimplemented` and `unreachable` in non-test code. Crates inherit the workspace lints and do not switch them off with local `allow` attributes.

### No unsafe

`unsafe` code is forbidden outside tests. Any exception is named and reviewed; kernel crates never get one.

### Bounded parsers

Every reader treats its input as hostile: it limits sizes and nesting, and it returns an error instead of guessing or crashing when input is malformed.

### Crash fixes come with a regression test

A fix for a crash, hang or panic lands together with a test that reproduces the failing input, so the same crash cannot come back unnoticed.
