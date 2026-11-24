;; Hostile module (CC-BY-4.0): imports a host memory (shared host state).
;; Expected: sandbox.forbidden_import (only function imports can be allowed).
(module
  (import "env" "memory" (memory 1))
  (func (export "_start")))
