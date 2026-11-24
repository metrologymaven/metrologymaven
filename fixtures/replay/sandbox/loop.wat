;; Hostile module (CC-BY-4.0): infinite loop. Expected: sandbox.budget_exhausted (fuel or
;; wall clock), process alive.
(module
  (func (export "_start")
    loop $forever
      br $forever
    end))
