;; Test kernel for TRUST.REPLAY.SANDBOX (CC-BY-4.0). A WASI command speaking the KRN.MAN.WASM
;; protocol: it reads the canonical call from stdin, requires algorithm `test.sandbox.fnv@1`,
;; and writes the envelope
;;   {"algorithm":"test.sandbox.fnv@1","fnv1a64":"<16 hex>","sqrt_sum":"<16 hex>"}
;; where fnv1a64 is FNV-1a 64 of the call bytes and sqrt_sum the f64 bits of
;; sqrt(sum over call bytes b of f64(b) * 0.1), summed in byte order. Exit 0.
;; Any other algorithm: {"error":"unknown_algorithm"}, exit 1. A call of 1 MiB or more:
;; {"error":"oversized_call"}, exit 2. Imports only allowlisted WASI functions.
(module
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
  (memory 32)
  (export "memory" (memory 0))
  (data (i32.const 64) "{\"algorithm\":\"test.sandbox.fnv@1\"")
  (data (i32.const 128) "{\"algorithm\":\"test.sandbox.fnv@1\",\"fnv1a64\":\"")
  (data (i32.const 192) "\",\"sqrt_sum\":\"")
  (data (i32.const 224) "\"}")
  (data (i32.const 256) "{\"error\":\"unknown_algorithm\"}")
  (data (i32.const 320) "{\"error\":\"oversized_call\"}")

  ;; Writes len bytes at ptr to stdout (one iovec at 0, count at 8) and exits with code.
  (func $emit (param $ptr i32) (param $len i32) (param $code i32)
    i32.const 0
    local.get $ptr
    i32.store
    i32.const 4
    local.get $len
    i32.store
    i32.const 1
    i32.const 0
    i32.const 1
    i32.const 8
    call $fd_write
    drop
    local.get $code
    call $proc_exit)

  ;; Writes the 16 lowercase hex digits of v at dst, most significant first.
  (func $hex64 (param $dst i32) (param $v i64) (local $i i32) (local $n i32)
    loop $next
      local.get $v
      i64.const 60
      local.get $i
      i32.const 2
      i32.shl
      i64.extend_i32_u
      i64.sub
      i64.shr_u
      i32.wrap_i64
      i32.const 15
      i32.and
      local.set $n
      local.get $dst
      local.get $i
      i32.add
      local.get $n
      i32.const 48
      i32.add
      local.get $n
      i32.const 87
      i32.add
      local.get $n
      i32.const 10
      i32.lt_u
      select
      i32.store8
      local.get $i
      i32.const 1
      i32.add
      local.tee $i
      i32.const 16
      i32.lt_u
      br_if $next
    end)

  (func $start (export "_start") (local $len i32) (local $n i32) (local $i i32) (local $h i64) (local $acc f64)
    ;; Read all of stdin to 65536.. (capacity 1 MiB).
    block $done
      loop $read
        i32.const 0
        i32.const 65536
        local.get $len
        i32.add
        i32.store
        i32.const 4
        i32.const 1048576
        local.get $len
        i32.sub
        i32.store
        i32.const 0
        i32.const 0
        i32.const 1
        i32.const 8
        call $fd_read
        br_if $done
        i32.const 8
        i32.load
        local.tee $n
        i32.eqz
        br_if $done
        local.get $len
        local.get $n
        i32.add
        local.tee $len
        i32.const 1048576
        i32.eq
        if
          i32.const 320
          i32.const 26
          i32.const 2
          call $emit
        end
        br $read
      end
    end
    ;; The call must start with {"algorithm":"test.sandbox.fnv@1".
    local.get $len
    i32.const 33
    i32.lt_u
    if
      i32.const 256
      i32.const 29
      i32.const 1
      call $emit
    end
    block $same
      loop $cmp
        local.get $i
        i32.const 33
        i32.eq
        br_if $same
        local.get $i
        i32.const 65536
        i32.add
        i32.load8_u
        local.get $i
        i32.const 64
        i32.add
        i32.load8_u
        i32.ne
        if
          i32.const 256
          i32.const 29
          i32.const 1
          call $emit
        end
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $cmp
      end
    end
    ;; FNV-1a 64 and the f64 sum over the call bytes.
    i64.const 0xcbf29ce484222325
    local.set $h
    i32.const 0
    local.set $i
    block $end
      loop $hash
        local.get $i
        local.get $len
        i32.ge_u
        br_if $end
        local.get $i
        i32.const 65536
        i32.add
        i32.load8_u
        local.set $n
        local.get $h
        local.get $n
        i64.extend_i32_u
        i64.xor
        i64.const 0x100000001b3
        i64.mul
        local.set $h
        local.get $acc
        local.get $n
        f64.convert_i32_u
        i64.const 0x3fb999999999999a
        f64.reinterpret_i64
        f64.mul
        f64.add
        local.set $acc
        local.get $i
        i32.const 1
        i32.add
        local.set $i
        br $hash
      end
    end
    ;; Envelope at 1024: prefix (45) + hex (16) + middle (14) + hex (16) + suffix (2) = 93 bytes.
    i32.const 1024
    i32.const 128
    i32.const 45
    memory.copy
    i32.const 1069
    local.get $h
    call $hex64
    i32.const 1085
    i32.const 192
    i32.const 14
    memory.copy
    i32.const 1099
    local.get $acc
    f64.sqrt
    i64.reinterpret_f64
    call $hex64
    i32.const 1115
    i32.const 224
    i32.const 2
    memory.copy
    i32.const 1024
    i32.const 93
    i32.const 0
    call $emit))
