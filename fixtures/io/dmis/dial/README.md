# IO.DMIS.DIAL test fixtures

- **Spec node:** `IO.DMIS.DIAL` (`fid-io-dmis::dial`, `import.dmis_list_dialects@1`)
- **License:** CC-BY-4.0 (synthetic clean-room profiles and DMIS fragment; zero vendor text copied)
- **Generator:** `python3 fixtures/io/dmis/dial/make_fixtures.py` (verify with `--check`)

## Files

- `test-vendor.toml` (AC-1, AC-2, AC-4): valid `fid.dmis-dialect` v1 profile (`test-vendor` `1.0.0`,
  `base = "dmis-5.3"`, relaxation `allow_lowercase_labels`, extra statement `VNDX/ON` -> `test-vendor.vndx_on`).
- `uses_vndx.dmi` (AC-2): minimal DMIS snippet containing `VNDX/ON` and a standard `FEAT/POINT`.
- `redefines_feat.toml` (AC-3): negative fixture redefining base DMIS major word `FEAT` (`base_redefinition`).
- `bad_shape.toml`: negative fixture with an invalid shape descriptor (`shape_compile_error`).
