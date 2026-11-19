---
id: IO.CAD.WORKER.PROTO
title: CAD worker protocol, framing and message validation
level: 4
parent: IO.CAD.WORKER
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-cad-proto]
satisfies: [REQ-IO.CAD.WORKER-003, REQ-IO.CAD.WORKER-004]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.WORKER.PROTO: CAD worker protocol, framing and message validation

## Purpose

The typed, versioned message protocol spoken between the document process and the CAD worker over the worker's stdin and stdout pipes: length-prefixed frames, a closed set of request and response kinds, hard size bounds and schema validation on every frame the document process receives. The source file travels as bytes inside requests, so the worker never needs a path.

## Scope

**In:**
- Frame format: 4-byte little-endian length, 1-byte kind, canonical payload (postcard or an equivalent compact, schema-fixed serde encoding [unverified: crate on the D-001 allowlist])
- Request kinds: `Hello`, `LoadStep` (with option `pmi`), `LoadIges`, `Shutdown`; response kinds: `HelloAck`, `Progress`, `Records` (a batch of typed B-rep, assembly or IGES entity records), `Report`, `Failed`
- Protocol version (major.minor) and the handshake rule (same major required)
- Per-frame and per-session byte caps, record-count caps, nesting caps, all charged to an `IO.REG.LIMITS` `Budget`
- Validation of every decoded record (finite floats, index ranges inside the session's record tables, enum ranges)

**Out:**
- Spawning, OS caps and killing (`IO.CAD.WORKER.HOST`)
- The worker executable and its sandbox (`IO.CAD.WORKER.BIN`)
- What the records mean (`IO.CAD.STEP`, `IO.CAD.IGES`)

## Requirements

- **REQ-IO.CAD.WORKER.PROTO-001** The protocol shall carry the source file only as bytes inside a load request and shall define no message that names a file-system path or network address.
- **REQ-IO.CAD.WORKER.PROTO-002** If a frame's declared length exceeds the per-frame cap (default 64 MiB) or the session total exceeds the session cap (default 8 GiB), then the decoder shall stop reading and return `reader_crashed{reason: frame_too_large}` without allocating the declared length.
- **REQ-IO.CAD.WORKER.PROTO-003** If a received frame does not decode to a known kind or fails record validation (non-finite float, out-of-range index, unknown enum value), then the decoder shall return `reader_crashed{reason: invalid_message}` naming the frame sequence number.
- **REQ-IO.CAD.WORKER.PROTO-004** When the handshake reports a protocol major version different from the host's, the client shall return `reader_crashed{reason: protocol_mismatch}` with both versions.
- **REQ-IO.CAD.WORKER.PROTO-005** Encoding then decoding any valid message shall return an equal message, and the encoding of a message shall be byte-identical across runs and platforms.

## Acceptance criteria

- **AC-1** Given every message kind built by `fixtures/io/cad/worker/proto/samples.rs` (seeded generator, seed 7, 1,000 messages), when each is encoded and decoded, then the decoded message equals the original and the encodings match the committed digests in `fixtures/io/cad/worker/proto/digests.txt` on Windows, Linux and macOS.
- **AC-2** (error path) Given a frame header declaring 2^31 bytes, when decoded with default caps, then the result is `reader_crashed{reason: frame_too_large}` and peak allocation stays below 1 MiB.
- **AC-3** (error path) Given a `Records` frame holding a face record whose loop index is out of range, then the result is `reader_crashed{reason: invalid_message, frame: n}`.
- **AC-4** (error path) Given a `HelloAck` with major version 2 against a host at major 1, then the result is `reader_crashed{reason: protocol_mismatch, host: "1.x", worker: "2.x"}`.
- **AC-5** Given the protocol fuzz target (registered with `IO.CORP.FUZZ`) run for the CI budget, then no panic and no allocation beyond the caps occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `Frame { kind: u8, payload: Vec<u8> }`; `Request`, `Response` enums; `Record` enum (`Surface`, `Curve`, `Pcurve`, `Vertex`, `Edge`, `Coedge`, `Loop`, `Face`, `Shell`, `Body`, `Product`, `Instance`, `PmiItem`, `IgesEntity`, `Skipped`), each with a source reference (`step_entity: u64` or `iges_de: u32`) and an optional untrusted name; `ProtoVersion { major: u16, minor: u16 }`; `ProtoLimits { frame_max, session_max, records_max }`.
- **Algorithm:** read 5-byte header, check against caps before allocating, read payload through `BoundedSource`, decode with a fixed schema, then validate records against per-session tables (indices refer only to records already received).
- **Errors:** `reader_crashed{reason: frame_too_large|invalid_message|protocol_mismatch|unexpected_eof}`, `limit_exceeded`.
- **Determinism:** D0 (fixed encoding, no maps with unstable order; `BTreeMap` only).
- **Performance budget:** decode + validate ≥ 500 MB/s of records on reference hardware [unverified until measured].
- **Crate:** `fid-cad-proto` (shared by host and worker; no I/O beyond `Read`/`Write` traits).

## Test fixtures

Seeded message generator and committed encoding digests under `fixtures/io/cad/worker/proto/` (CC-BY-4.0); hostile frames from `IO.CORP.HOSTILE` style generation (hand-written here).

## Build prompt

> Create crate `fid-cad-proto`: the CAD worker protocol. Frames are `u32` LE length + `u8` kind + payload encoded with a fixed-schema compact serde format (pick one on the D-001 allowlist; record it). Requests: `Hello{version}`, `LoadStep{bytes, options}`, `LoadIges{bytes, options}`, `Shutdown`. Responses: `HelloAck{version, reader, reader_version}`, `Progress`, `Records{seq, Vec<Record>}`, `Report`, `Failed{code, detail}`. No message may carry a path or address. Check the declared length against `frame_max` (64 MiB) and `session_max` (8 GiB) before allocating → `reader_crashed{reason: frame_too_large}`. Validate every record (finite f64, indices only to earlier records, enum ranges) → `reader_crashed{reason: invalid_message, frame}`. Handshake major mismatch → `reader_crashed{reason: protocol_mismatch}`. Use `IO.REG.LIMITS` `Budget` and `BoundedSource` from `fid-io-reg`. Encodings must be byte-identical across platforms (commit digests). CONSTITUTION §8: no panic, no unwrap, no unsafe; BTreeMap only. Add a fuzz target. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
