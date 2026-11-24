;; Hostile module (CC-BY-4.0): answers outside the KRN.MAN.WASM protocol. The first byte of
;; standard input selects the behaviour:
;;   "0" writes a valid-looking envelope to stderr (fd 2) and exits 0
;;   "1" writes a valid-looking envelope to stdout and exits 7
;;   "2" exits 0 with nothing on stdout
;;   "3" writes 64 KiB to stdout forever
;; Expected: sandbox.trap/unexpected_exit for 0..2; sandbox.memory_limit (output cap) for 3.
(module
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory 2)
  (data (i32.const 64) "{\"algorithm\":\"x\"}")
  (func $write (param $fd i32) (param $ptr i32) (param $len i32)
    i32.const 0
    local.get $ptr
    i32.store
    i32.const 4
    local.get $len
    i32.store
    local.get $fd
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_write
    drop)
  (func (export "_start")
    i32.const 0
    i32.const 100
    i32.store
    i32.const 4
    i32.const 1
    i32.store
    i32.const 0
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_read
    drop
    block $flood
      block $silent
        block $seven
          block $stderr
            i32.const 100
            i32.load8_u
            i32.const 48
            i32.sub
            br_table $stderr $seven $silent $flood $silent
          end
          i32.const 2
          i32.const 64
          i32.const 17
          call $write
          return
        end
        i32.const 1
        i32.const 64
        i32.const 17
        call $write
        i32.const 7
        call $proc_exit
        return
      end
      i32.const 0
      call $proc_exit
      return
    end
    loop $again
      i32.const 1
      i32.const 0
      i32.const 65536
      call $write
      br $again
    end))
