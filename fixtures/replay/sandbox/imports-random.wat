;; Hostile module (CC-BY-4.0): imports the WASI randomness source random_get.
;; Expected: sandbox.forbidden_import.
(module
  (import "wasi_snapshot_preview1" "random_get" (func (param i32 i32) (result i32)))
  (func (export "_start")))
