---
id: IO.REG.LIMITS
title: Reader limit harness, error taxonomy and untrusted strings
level: 3
parent: IO.REG
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-reg]
satisfies: [REQ-IO.REG-004, REQ-IO.REG-005, REQ-IO.REG-006, REQ-IO.REG-015]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.REG.LIMITS: Reader limit harness, error taxonomy and untrusted strings

## Purpose

The library every reader runs inside: a per-invocation `Budget` (bytes, elements, nesting depth, allocation, wall time), a bounded byte source, the shared typed error enum, and the rule that every string taken from a file is wrapped as untrusted text. It turns "parsers are bounded and fail closed" (CONSTITUTION §8) into one API.

## Scope

**In:**
- `Limits` with documented defaults and per-call overrides (only lower, unless the principal is human)
- `Budget` counters and the `BoundedSource` reader wrapper
- The `IoError` taxonomy with byte offset or line number
- `UntrustedText` wrapping of file-derived strings (C6), reusing the CORE type when it exists
- A `catch`-free contract: readers return errors; the harness never relies on unwinding

**Out:**
- Format detection (`IO.REG.DETECT`)
- Transaction and all-or-nothing commit (`IO.REG.ENV`)
- Process isolation for native-code readers (`IO.CAD.WORKER`, `IO.REG.PLUGIN`)

## Requirements

- **REQ-IO.REG.LIMITS-001** The harness shall enforce per-invocation limits on input bytes, element count, nesting depth, total tracked allocation and wall time, with defaults 64 GiB, 2^32 elements, depth 64, 8 GiB and 600 s [unverified: defaults to tune on the corpus].
- **REQ-IO.REG.LIMITS-002** When a reader charges a budget past its limit, the harness shall return error `limit_exceeded` naming the limit, its configured value and the byte offset reached.
- **REQ-IO.REG.LIMITS-003** If the principal is an agent or a script, then a request to raise any limit above its default shall fail with error `limit_raise_forbidden`.
- **REQ-IO.REG.LIMITS-004** The `IoError` type shall be a closed enum whose malformed-input variants each carry a byte offset or a 1-based line and column.
- **REQ-IO.REG.LIMITS-005** Every string a reader extracts from file content shall be returned as `UntrustedText`, and the crate shall offer no API that converts it to an instruction or command string.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/limits/deep_nest.json` (nesting depth 10,000) and a toy recursive-descent reader built on the harness, when it reads the file with default limits, then it returns `limit_exceeded{limit: depth, value: 64}` without stack overflow.
- **AC-2** Given a `BoundedSource` over `fixtures/io/reg/limits/zeros_2mib.bin` with a byte limit of 1 MiB, when read to end, then it returns `limit_exceeded{limit: bytes, value: 1048576}` at offset 1,048,576.
- **AC-3** Given an element limit of 1,000 and a reader charging one element per line of `fixtures/io/reg/limits/lines_5000.txt`, when read, then it fails at line 1,001.
- **AC-4** (error path) Given principal kind `agent` and a requested byte limit of 128 GiB, when the budget is created, then it fails with `limit_raise_forbidden`.
- **AC-5** Given the fuzz target `limits_toy_reader` run for 5 CPU-minutes, then no panic, abort or timeout occurs.
- **AC-6** A compile-time test shows `UntrustedText` has no `Deref<Target=str>` and no `Into<String>`; only explicit `as_display()` and `len()` are offered.

## Design

- **Registry commands:** none (library).
- **Data structures:** `Limits { bytes, elements, depth, alloc, wall }`; `Budget` (counters, `charge_bytes`, `charge_elements`, `enter()/leave()` depth guard returning a scope token, `charge_alloc`); `BoundedSource<R: Read>`; `IoError` enum (`limit_exceeded`, `malformed{at}`, `truncated_file{at}`, `unit_required`, `format_unknown`, `format_ambiguous`, `format_mismatch`, `format_duplicate`, `raw_path_forbidden`, `path_outside_root`, `target_exists`, `limit_raise_forbidden`, `reader_crashed`, `io{kind}`); `Position { byte: u64, line: Option<u32>, col: Option<u32> }`.
- **Algorithm:** allocation is tracked by explicit `charge_alloc` calls before each growth of a reader's buffers (no global allocator hook). Wall time is checked on each charge with a monotonic clock read every 4,096 charges.
- **Errors:** the enum above; error codes are stable snake_case strings used across CLI, MCP and journal.
- **Determinism:** D0 for every outcome except wall-time expiry, which is `N` and reported as such.
- **Performance budget:** harness overhead < 3% on a 1 GiB sequential read [unverified until measured].
- **Crate:** `fid-io-reg`.

## Test fixtures

Generated at test time by a seeded script: deep-nesting JSON, 2 MiB of zeros, 5,000-line text. No third-party data.

## Build prompt

> In crate `fid-io-reg`, implement the reader harness: `Limits` (defaults 64 GiB bytes, 2^32 elements, depth 64, 8 GiB tracked allocation, 600 s wall), `Budget` with `charge_bytes/charge_elements/charge_alloc/enter` (depth guard), `BoundedSource<R>`, the closed `IoError` enum with `Position { byte, line, col }` on malformed-input variants, and `UntrustedText` (no Deref to str, no Into<String>; reuse the CORE type if it exists). Raising a limit above default fails `limit_raise_forbidden` when the principal is an agent or script. Exceeding a limit returns `limit_exceeded{limit, value, at}`. CONSTITUTION §8: no panic, no unsafe, no unwrap/expect/todo/unreachable; all input hostile. Use no std HashMap. Tests AC-1..AC-6 plus a cargo-fuzz target `limits_toy_reader`.

## Open questions

- Whether `UntrustedText` lives in CORE or here; CORE.PRJ types user metadata as untrusted but no CORE leaf yet names the type. This leaf reuses CORE's if present and otherwise defines it for promotion later.

## Children

_Leaf: none._
