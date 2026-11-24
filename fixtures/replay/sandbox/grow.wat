;; Hostile module (CC-BY-4.0): unbounded memory growth, one page at a time, ignoring results.
;; Expected: sandbox.memory_limit when growth passes the cap, process alive.
(module
  (memory 1)
  (func (export "_start")
    loop $more
      i32.const 1
      memory.grow
      drop
      br $more
    end))
