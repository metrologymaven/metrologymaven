;; Hostile module (CC-BY-4.0): links the sealed clock (module fid_trap, as `fid-wasm-check seal`
;; writes it) and calls it. Expected: sandbox.trap with kind host_trap; no clock is read.
(module
  (import "fid_trap" "clock_time_get" (func $clock (param i32 i64 i32) (result i32)))
  (memory 1)
  (func (export "_start")
    i32.const 0
    i64.const 1
    i32.const 16
    call $clock
    drop))
