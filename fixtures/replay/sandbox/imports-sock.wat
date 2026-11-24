;; Hostile module (CC-BY-4.0): imports the WASI socket function sock_recv.
;; Expected: sandbox.forbidden_import.
(module
  (import "wasi_snapshot_preview1" "sock_recv" (func (param i32 i32 i32 i32 i32 i32) (result i32)))
  (func (export "_start")))
