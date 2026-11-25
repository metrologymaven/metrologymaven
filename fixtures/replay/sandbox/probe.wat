;; Hostile module (CC-BY-4.0): probes the host. It asks for the environment size, reads from
;; fd 3 and writes to fd 5, and reports what it got as one digit each:
;;   {"algorithm":"probe","results":"<env count><env bytes><fd_read(3) errno><fd_write(5) errno>"}
;; Expected envelope: results "0088" (empty environment; EBADF = 8 for every fd but 0, 1, 2).
(module
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_sizes_get" (func $env_sizes (param i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "environ_get" (func $env_get (param i32 i32) (result i32)))
  (memory 1)
  (data (i32.const 64) "{\"algorithm\":\"probe\",\"results\":\"????\"}")
  (func $digit (param $at i32) (param $v i32)
    local.get $at
    i32.const 48
    local.get $v
    i32.add
    i32.const 63
    local.get $v
    i32.const 10
    i32.lt_u
    select
    i32.store8)
  (func (export "_start")
    ;; poison the result slots so a missing write shows
    i32.const 200
    i32.const 7
    i32.store
    i32.const 204
    i32.const 7
    i32.store
    i32.const 200
    i32.const 204
    call $env_sizes
    drop
    i32.const 300
    i32.const 400
    call $env_get
    drop
    i32.const 96
    i32.const 200
    i32.load
    call $digit
    i32.const 97
    i32.const 204
    i32.load
    call $digit
    ;; iovec at 0 -> 16 bytes at 500
    i32.const 0
    i32.const 500
    i32.store
    i32.const 4
    i32.const 16
    i32.store
    i32.const 98
    i32.const 3
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_read
    call $digit
    i32.const 99
    i32.const 5
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_write
    call $digit
    ;; write the 38-byte envelope at 64 to stdout
    i32.const 0
    i32.const 64
    i32.store
    i32.const 4
    i32.const 38
    i32.store
    i32.const 1
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_write
    drop))
