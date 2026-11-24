;; Hostile module (CC-BY-4.0): unbounded recursion. Expected: sandbox.trap with kind
;; call_stack_exhausted; the host stack is never used for guest calls.
(module
  (func $down (param i64) (result i64)
    local.get 0
    i64.const 1
    i64.add
    call $down)
  (func (export "_start")
    i64.const 0
    call $down
    drop))
