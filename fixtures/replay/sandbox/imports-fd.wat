;; Hostile module (CC-BY-4.0): imports the WASI file function path_open.
;; Expected: sandbox.forbidden_import at load, never instantiated.
(module
  (import "wasi_snapshot_preview1" "path_open"
    (func $path_open (param i32 i32 i32 i32 i32 i64 i64 i32 i32) (result i32)))
  (func (export "_start")))
