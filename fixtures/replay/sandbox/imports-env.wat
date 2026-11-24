;; Hostile module (CC-BY-4.0): imports a host function from module "env" (environment access).
;; Expected: sandbox.forbidden_import.
(module
  (import "env" "getenv" (func (param i32) (result i32)))
  (func (export "_start")))
