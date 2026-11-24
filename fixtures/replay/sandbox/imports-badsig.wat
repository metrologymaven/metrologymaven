;; Hostile module (CC-BY-4.0): imports the allowlisted fd_write with a signature that is not
;; the WASI preview-1 one. Expected: sandbox.forbidden_import.
(module
  (import "wasi_snapshot_preview1" "fd_write" (func (param i64) (result i32)))
  (func (export "_start")))
