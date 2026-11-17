# Out of scope

Seed rows matched here are excluded from the coverage requirement. Every row needs a reason. Reopening one takes a decision record.

| Pattern | Reason |
|---|---|
| `GOM.ACQ.*` | Scanner acquisition is vendor hardware. We read scanners' exported data and integrate open SDKs through HAL instead (Design Baseline §8; R01 §7). |
| `GOM.COR.*` | Digital image correlation and strain analysis are a different product category. |
| `GOM.CT.*` | CT volume analysis is deferred. It is a good candidate for a community plugin after P4. Reopen with a decision record. |
| `PW.MOD.*` | Reverse-engineering surface modelling is outside inspection. It could become a separate project. |
