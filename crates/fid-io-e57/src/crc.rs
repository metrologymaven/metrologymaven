//! CRC-32C (Castagnoli): reflected polynomial 0x82F63B78, initial value and final XOR
//! 0xFFFFFFFF. Table-driven, eight bytes per step ("slicing by 8"), tables built at
//! compile time.

const POLY: u32 = 0x82F6_3B78;

const fn make_tables() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
            k += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut i = 0;
    while i < 256 {
        let mut s = 1;
        while s < 8 {
            let prev = t[s - 1][i];
            t[s][i] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            s += 1;
        }
        i += 1;
    }
    t
}

static TABLES: [[u32; 256]; 8] = make_tables();

/// Feeds `data` into a running CRC-32C register. Start with `0xFFFF_FFFF` and invert the
/// final register (`!reg`) to get the checksum; [`crc32c`] does both.
pub fn crc32c_update(mut reg: u32, data: &[u8]) -> u32 {
    let t = &TABLES;
    let mut chunks = data.chunks_exact(8);
    for c in &mut chunks {
        let lo = reg ^ u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        reg = t[7][(lo & 0xff) as usize]
            ^ t[6][((lo >> 8) & 0xff) as usize]
            ^ t[5][((lo >> 16) & 0xff) as usize]
            ^ t[4][(lo >> 24) as usize]
            ^ t[3][c[4] as usize]
            ^ t[2][c[5] as usize]
            ^ t[1][c[6] as usize]
            ^ t[0][c[7] as usize];
    }
    for &b in chunks.remainder() {
        reg = (reg >> 8) ^ t[0][((reg ^ u32::from(b)) & 0xff) as usize];
    }
    reg
}

/// The CRC-32C of `data`.
pub fn crc32c(data: &[u8]) -> u32 {
    !crc32c_update(0xFFFF_FFFF, data)
}

#[cfg(test)]
mod tests {
    use super::crc32c;

    #[test]
    fn check_value() {
        // The published CRC-32C check value of the ASCII digits "123456789".
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
    }
}
