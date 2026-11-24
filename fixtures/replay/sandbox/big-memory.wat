;; Hostile module (CC-BY-4.0): declares 4 GiB (65536 pages) of initial linear memory and
;; touches its last byte. Expected: sandbox.memory_limit at instantiation whenever the cap is
;; below 4 GiB; under the default 4 GiB cap it runs, allocating only the touched page.
(module
  (memory 65536)
  (func (export "_start")
    i32.const -1
    i32.const 1
    i32.store8))
