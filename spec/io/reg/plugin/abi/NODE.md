---
id: IO.REG.PLUGIN.ABI
title: Versioned format-plugin contract and descriptor
level: 4
parent: IO.REG.PLUGIN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG.PLUGIN-001]
covers: []
depends_on: [IO.REG.DETECT, IO.REG.LIMITS, IO.REG.ENV.IMPORT, IO.REG.ENV.EXPORT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-io-plugin]
updated: 2026-11-22
---

# IO.REG.PLUGIN.ABI: Versioned format-plugin contract and descriptor

## Purpose

The published contract every format plugin and every contract-based first-party reader implements: a descriptor, a read entry point (bounded byte stream and options in, a stream of typed neutral records out), a write entry point, and major-version negotiation. One contract is expressed twice, as a WASM component interface and as a length-prefixed message protocol for out-of-process native hosts, so the two hosts cannot drift. It ships in P1a with its first user, the drawing reader (`IO.REG.PLUGIN.DRAW`).

## Scope

**In:**
- `PluginDescriptor`: plugin ID, version, contract major/minor, formats (format ID, direction, signatures, extensions, options JSON Schema), requested caps (memory, wall time)
- Neutral record types for read output: points, meshes, QIF fragments (as QIF-JSON lossless), drawing pages, tables, untrusted strings, warnings
- WIT-style interface file and the equivalent message schema (one source, both generated)
- Version negotiation and `plugin_abi_mismatch`
- Adapters that turn a contract reader or writer into the `Reader`/`Writer` traits of the import and export skeletons

**Out:**
- Running plugins (`IO.REG.PLUGIN.WASM`, `IO.REG.PLUGIN.NATIVE`)
- Output validation and provenance (`IO.REG.PLUGIN.VALID`)
- Packaging, signing and install (ECO; install is A0)

## Requirements

- **REQ-IO.REG.PLUGIN.ABI-001** The contract shall be published as one versioned source file from which both the WASM interface and the native message schema are generated, and CI shall fail if either generated form is out of date.
- **REQ-IO.REG.PLUGIN.ABI-002** If a plugin declares a contract major version different from the host's, then loading shall fail with `plugin_abi_mismatch` naming both versions.
- **REQ-IO.REG.PLUGIN.ABI-003** A higher minor version in the host than in the plugin shall be accepted, and a plugin with a higher minor version than the host shall be rejected with `plugin_abi_mismatch`.
- **REQ-IO.REG.PLUGIN.ABI-004** Every string a plugin returns shall be typed as untrusted content by the adapter (CONSTITUTION C6).
- **REQ-IO.REG.PLUGIN.ABI-005** The adapter shall register each declared format with `IO.REG.DETECT` with origin `plugin`, or `first_party` for contract readers shipped with the product.
- **REQ-IO.REG.PLUGIN.ABI-006** The read entry point shall receive only a bounded byte stream and validated options, and the contract shall define no call that reaches the file system, network, clock or document.

## Acceptance criteria

- **AC-1** Given the contract source, when the generators run, then the WIT file and message schema equal the committed files, and CI fails `abi_generated_out_of_date` after an edit to only one of them.
- **AC-2** (error path) Given a test descriptor with major version N+1, then loading fails `plugin_abi_mismatch`.
- **AC-3** Given a test plugin at minor 0 and host at minor 1, then it loads; given plugin minor 2 and host minor 1, then `plugin_abi_mismatch`.
- **AC-4** Given an in-process test reader implementing the contract, when registered and used by `import.file@1`, then `import.list_formats@1` shows its format and the import succeeds with all strings typed untrusted.
- **AC-5** Given the contract source, then a review script finds no import of file, socket, clock or document capabilities in the interface.

## Design

- **Registry commands:** none (contract and adapters).
- **Data structures:** `PluginDescriptor`, `ReadRequest { options, stream }`, `Record` enum (`Points`, `Mesh`, `QifJson`, `DrawingPage`, `Table`, `Warning`), `WriteRequest`, `ContractVersion { major, minor }`.
- **Algorithm:** contract source in a small IDL file; generator emits WIT and a length-prefixed message schema; adapters map `Record`s into the `StageSink`.
- **Errors:** `plugin_abi_mismatch`, `abi_generated_out_of_date` (CI), `plugin_descriptor_invalid`.
- **Determinism:** D0.
- **Performance budget:** adapter overhead < 5% of reader time for 10^7 points [unverified until measured].
- **Crate:** `fid-io-plugin` (module `abi`).

## Test fixtures

An in-process test reader and descriptors under `fixtures/io/reg/plugin/abi/` (CC-BY-4.0).

## Build prompt

> Create crate `fid-io-plugin`, module `abi`: one IDL source for the format-plugin contract (descriptor with formats, signatures, options schema and requested caps; read entry with a bounded byte stream and options returning `Record`s: points, mesh, QIF-JSON lossless fragments, drawing pages, tables, warnings; write entry; contract version major/minor), generators for a WIT interface and a length-prefixed message schema with a CI freshness check, version negotiation (`plugin_abi_mismatch`: different major, or plugin minor above host), and adapters to the `fid-io-reg` `Reader`/`Writer` traits that type every returned string as untrusted and register formats with origin `plugin` or `first_party`. No file, network, clock or document capability in the contract. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- Phase: this leaf is P1a because the P1a drawing reader is built against it (agreed at review); the hosts that run third-party plugins stay P1b.

## Children

_Leaf: none._
