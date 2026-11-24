;; Hostile module (CC-BY-4.0): one trap per entry. The `_start` entry dispatches on the first
;; byte of standard input ("0".."6") through a br_table. Expected traps, by digit:
;;   0 unreachable, 1 out_of_bounds_memory_access, 2 integer_divide_by_zero,
;;   3 integer_overflow, 4 bad_conversion_to_integer, 5 indirect_call_to_null,
;;   6 bad_signature
(module
  (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
  (type $v_i (func (result i32)))
  (memory 1)
  (table 2 funcref)
  (elem (i32.const 0) $nothing)
  (func $nothing)
  (func (export "_start")
    ;; read one byte to 100
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
    block $sig
      block $null
        block $conv
          block $ovf
            block $div
              block $oob
                block $unr
                  i32.const 100
                  i32.load8_u
                  i32.const 48
                  i32.sub
                  br_table $unr $oob $div $ovf $conv $null $sig $sig
                end
                unreachable
              end
              i32.const -1
              i32.load
              drop
              return
            end
            i32.const 1
            i32.const 0
            i32.div_u
            drop
            return
          end
          i32.const 0x80000000
          i32.const -1
          i32.div_s
          drop
          return
        end
        i32.const 0x7fc00000
        f32.reinterpret_i32
        i32.trunc_f32_s
        drop
        return
      end
      i32.const 1
      call_indirect (type $v_i)
      drop
      return
    end
    i32.const 0
    call_indirect (type $v_i)
    drop))
