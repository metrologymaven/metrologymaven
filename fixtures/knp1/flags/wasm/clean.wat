(module
  (memory 1)
  (func (param v128 v128) (result v128)
    local.get 0
    local.get 1
    f64x2.add
    v128.const i8x16 0xfd 0x87 0x02 0 0 0 0 0 0 0 0 0 0 0 0 0
    f64x2.mul)
  (func (param i32) (result f64) (local f64)
    block
      local.get 0
      br_table 0 0
    end
    local.get 0
    i32.load
    drop
    i64.const 0x83fd  ;; LEB bytes fd 87 02
    drop
    f64.const 0x1.00000000287fdp+0  ;; bytes fd 87 02 00 00 00 f0 3f
  ))
