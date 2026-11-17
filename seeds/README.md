# Seeds: competitor feature inventories

Each file lists one incumbent's functional areas as rows with stable IDs. Together they are the **coverage checklist**: `tools/scope.py check` fails if any row is neither covered by a spec node (`covers:`) nor listed in [out-of-scope.md](out-of-scope.md).

## Row format

```
| GOM.ALN.04 | 3-2-1 alignment | notes |
```

- ID: `<PREFIX>.<AREA>.<NN>`. PREFIX and AREA are uppercase letters only; NN is two or three digits. Never renumber or reuse an ID. To retire a row, keep it and add it to out-of-scope.md with the reason.
- Describe each feature in your own words. Never copy vendor documentation text.
- Notes: mark `[unverified]` until someone who has used the software confirms the row.

## Files

| Prefix | File | Product | Verified |
|---|---|---|---|
| GOM | [gom-inspect.md](gom-inspect.md) | ZEISS INSPECT Optical 3D (formerly GOM Inspect) | no |
| PW | [polyworks.md](polyworks.md) | PolyWorks Inspector (+ Modeler, DataLoop) | no |
| PCD | [pc-dmis.md](pc-dmis.md) | PC-DMIS | no |
| SA | [spatialanalyzer.md](spatialanalyzer.md) | SpatialAnalyzer | no |
| DEV | [devices.md](devices.md) | Device support catalog (instruments we must support; owned by HAL) | no |
| JOB | [journeys.md](journeys.md) | Personas, acceptance jobs and usability bars (owned by FLOW) | no |

## Wanted: more inventories

CALYPSO (`CAL`), Geomagic Control X (`GCX`), Verisurf (`VS`), Metrolog X4 (`MLX`), CAMIO (`CAM`), MCOSMOS (`MCO`), Creaform VXelements (`VXE`), Polyworks|Talisman (`PWT`). Add one file per product and a row in the table above.
