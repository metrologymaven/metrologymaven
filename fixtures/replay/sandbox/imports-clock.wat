;; Hostile module (CC-BY-4.0): imports the unsealed WASI clock clock_time_get.
;; Expected: sandbox.forbidden_import (only the sealed fid_trap form links).
(module
  (import "wasi_snapshot_preview1" "clock_time_get" (func (param i32 i64 i32) (result i32)))
  (func (export "_start")))
