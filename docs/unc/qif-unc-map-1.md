# QIF 3.0 Uncertainty Statement Mapping Table (`qif-unc-map@1`)

Spec node: `UNC.UEC.QIF` · Algorithm ID: `unc.uec.qif_map@1.0.0` · Extension version: `unc-qif-stmt@1` · Namespace: `https://metrologymaven.org/ns/qif-ext/v1` (prefix `fx`).

Every `UncertaintyStatement` is carried in full as one `fx:UncertaintyStatement` element inside QIF 3.0's generic user-data extension hook (`Attributes/AttributeUserData/UserDataXML`). For UEC-1..3 statements, numeric values that QIF 3.0 can represent natively are also written into native fields for readers that ignore extensions. For UEC-0 statements, no native numeric uncertainty field is written.

Clause references cite ISO 23952:2020 designators and clause numbers only; no standards text is reproduced. Native element names are `[unverified]` against the full pinned QIF 3.0 XSD until `IO.QIF.GEN.PIN` schemas are fetched in CI.

| Statement field | QIF 3.0 native element | Extension element (`UserDataXML/...`) | ISO 23952:2020 reference |
|---|---|---|---|
| `class` | — | `fx:UncertaintyStatement/fx:Class` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `uec0_reason` | — | `fx:UncertaintyStatement/fx:Uec0Reason` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `method` | — | `fx:UncertaintyStatement/fx:Method` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `unit` | `LinearUnit` / `AngularUnit` `[unverified]` | `fx:UncertaintyStatement/fx:Unit` | ISO 23952:2020 cl. 6.5 (`Units`) |
| `u_std` | `CombinedUncertainty` `[unverified]` | `fx:UncertaintyStatement/fx:UStd` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `expanded.U` | `ExpandedUncertainty` `[unverified]` | `fx:UncertaintyStatement/fx:Expanded/fx:U` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `expanded.k` | `CoverageFactor` `[unverified]` | `fx:UncertaintyStatement/fx:Expanded/fx:K` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `interval.low` | `LowerBound` `[unverified]` | `fx:UncertaintyStatement/fx:Interval/fx:Low` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `interval.high` | `UpperBound` `[unverified]` | `fx:UncertaintyStatement/fx:Interval/fx:High` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `interval.p` | `CoverageProbability` `[unverified]` | `fx:UncertaintyStatement/fx:Interval/fx:P` | ISO 23952:2020 cl. 6.8 (`Results / MeasurementUncertainty`) |
| `interval.kind` | — | `fx:UncertaintyStatement/fx:Interval/fx:Kind` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `model` | — | `fx:UncertaintyStatement/fx:Model` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `sources` | — | `fx:UncertaintyStatement/fx:Sources/fx:Source` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `contributors` | — | `fx:UncertaintyStatement/fx:Contributors/fx:Contributor` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `lpu_validated_by_mc` | — | `fx:UncertaintyStatement/fx:LpuValidatedByMc` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `nonlinearity` | — | `fx:UncertaintyStatement/fx:Nonlinearity` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `selection_rule` | — | `fx:UncertaintyStatement/fx:SelectionRule` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `estimate_convention` | — | `fx:UncertaintyStatement/fx:EstimateConvention` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |
| `provenance` | — | `fx:UncertaintyStatement/fx:Provenance` | ISO 23952:2020 cl. 6.3 (`Attributes/UserDataXML`) |

## Import rules

1. **Extension present:** `fx:UncertaintyStatement` is authoritative. Every present native numeric value must agree with the extension within 1 ulp (`qif_unc_conflict{field}` otherwise). UEC-0 statements must carry no native numeric uncertainty field.
2. **Native values only (no extension):** imported as UEC-1, method `DECLARED`, source kind `declared` with `id = "qif_import"` via `UNC.UEC.CLASS` (`uec::assign`); never upgraded to UEC-2 or UEC-3.
3. **Bounds and versioning:** `ext_xml` is capped at 1 MiB and XML nesting depth 16 (`limit_exceeded`); any `version` other than `unc-qif-stmt@1` fails with `unsupported_extension`.
