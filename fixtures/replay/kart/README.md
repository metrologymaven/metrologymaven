# TRUST.REPLAY.KART.LOOKUP test fixtures

- **Spec node:** `TRUST.REPLAY.KART.LOOKUP` (`fid-vreplay::kart`)
- **License:** CC-BY-4.0 (synthetic stub WASM module and `fid-pkg` test containers)
- **Generator:** `python3 fixtures/replay/kart/make_fixtures.py` (verify with `--check`)

## Files

- `stub-kernel.wasm`: minimal valid 23-byte WebAssembly stub module.
- `kernel-manifest-ok.json`: `KRN.MAN.MANI.SCHEMA` v1 manifest (`semver = 0.1.0`) referencing `stub-kernel.wasm`.
- `kernel-manifest-9.9.9.json`: manifest for unavailable kernel `9.9.9` (AC-4).
- `kernel-manifest-no-wasm.json`: manifest listing no `wasm32-wasip1` artifact (REQ-003).
- `points-with-kernel.fidpkg`: genuine `fid-pkg` container embedding `kernel/<sha256>.wasm` (AC-1).
- `points-no-kernel.fidpkg`: genuine `fid-pkg` container without embedded kernel (AC-2).
