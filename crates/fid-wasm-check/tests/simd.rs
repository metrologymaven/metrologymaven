//! No SIMD in the kernel artifact (spec node KRN.MAN.REPRO, FMA guard; #318 review item 3).
//!
//! Modules are hand-assembled from the binary format; offsets and sub-opcodes are counted by
//! hand from those bytes.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_wasm_check::simd::{simd_opcodes, SimdHit};

/// header (8) + type section `() -> ()` (6 bytes, 8..14) + function section with one entry per
/// body (14..) + code section with the given bodies (each without its size prefix, < 128 bytes).
fn module(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut m = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
    m.extend_from_slice(&[1, 4, 0x01, 0x60, 0x00, 0x00]);
    let mut funcs = vec![bodies.len() as u8];
    funcs.extend(std::iter::repeat(0u8).take(bodies.len()));
    m.extend_from_slice(&[3, funcs.len() as u8]);
    m.extend_from_slice(&funcs);
    let mut code = vec![bodies.len() as u8];
    for b in bodies {
        code.push(b.len() as u8);
        code.extend_from_slice(b);
    }
    m.extend_from_slice(&[10, code.len() as u8]);
    m.extend_from_slice(&code);
    m
}

/// `i32.const 125` (signed LEB128 0xFD 0x00: an 0xFD byte inside an immediate), drop, end.
fn scalar() -> Vec<u8> {
    vec![0x00, 0x41, 0xfd, 0x00, 0x1a, 0x0b]
}

#[test]
fn scalar_code_passes_even_with_an_fd_byte_in_an_immediate() {
    assert_eq!(simd_opcodes(&module(&[scalar()])).unwrap(), []);
}

#[test]
fn v128_const_is_refused() {
    // locals, v128.const (0xFD 0x0C + 16 bytes), drop, end.
    let mut b = vec![0x00, 0xfd, 0x0c];
    b.extend_from_slice(&[0u8; 16]);
    b.extend_from_slice(&[0x1a, 0x0b]);
    // One body: function section 14..18, code section id 18, size 19, count 20, body size 21,
    // locals byte 22, the 0xFD prefix at 23.
    assert_eq!(simd_opcodes(&module(&[b])).unwrap(), [SimdHit { offset: 23, func: 0, sub: 0x0c }]);
}

#[test]
fn relaxed_madd_is_refused_in_the_second_body() {
    // f32x4.relaxed_madd is 0xFD 0x105 (unsigned LEB128 0x85 0x02).
    let b = vec![0x00, 0xfd, 0x85, 0x02, 0x0b];
    let m = module(&[scalar(), b]);
    // Two bodies: function section 14..19 (5 bytes), code id 19, size 20, count 21,
    // first body size 22 + 6 bytes (23..29), second body size 29, locals 30, prefix 31.
    let hits = simd_opcodes(&m).unwrap();
    assert_eq!(hits, [SimdHit { offset: 31, func: 1, sub: 0x105 }]);
    assert!(hits[0].to_string().contains("relaxed SIMD"));
}

#[test]
fn undecodable_code_fails_closed() {
    // 0xFF is not an opcode.
    assert!(simd_opcodes(&module(&[vec![0x00, 0xff, 0x0b]])).is_err());
    // A body that does not end on its final `end`.
    assert!(simd_opcodes(&module(&[vec![0x00, 0x0b, 0x01]])).is_err());
    // Not a module.
    assert!(simd_opcodes(b"\0asm").is_err());
}
