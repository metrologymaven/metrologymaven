//! Point data record formats 0 to 10: which fields each holds and where (ASPRS LAS 1.4,
//! point data records section; cited by designator only).
//!
//! Formats 0 to 5 share a 20-byte core: three 32-bit integer coordinates, a 16-bit
//! intensity, one byte of return bits (return number and number of returns in 3 bits
//! each, then the scan direction and edge-of-flight-line flags), one byte holding a 5-bit
//! class and three class flags, an 8-bit scan angle rank, user data and a 16-bit point
//! source ID. Formats 6 to 10 share a 30-byte core: the coordinates and intensity, one
//! byte with 4-bit return number and number of returns, one byte with four class flags,
//! a 2-bit scanner channel and the two scan flags, then a full class byte, user data, a
//! 16-bit scan angle, the point source ID and the GPS time. Optional groups follow at
//! fixed places: GPS time (8 bytes), RGB (three 16-bit values), NIR (16 bits) and the
//! 29-byte wave packet group. Any bytes past the format's size are extra bytes.

/// Size of a wave packet group in a point record.
pub const WAVE_PACKET_LEN: usize = 29;

/// A point data record format ID, 0 to 10.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointFormat(u8);

impl PointFormat {
    /// Format 6, the default for 1.4 output.
    pub const FORMAT_6: PointFormat = PointFormat(6);
    /// Format 7 (format 6 with RGB).
    pub const FORMAT_7: PointFormat = PointFormat(7);

    /// The format, if `id` is 0 to 10.
    pub fn new(id: u8) -> Option<PointFormat> {
        (id <= 10).then_some(PointFormat(id))
    }

    /// The format ID.
    pub fn id(self) -> u8 {
        self.0
    }

    /// Every format, 0 to 10.
    pub fn all() -> impl Iterator<Item = PointFormat> {
        (0..=10).map(PointFormat)
    }

    /// The record size of the format without extra bytes.
    pub fn base_len(self) -> u16 {
        match self.0 {
            0 => 20,
            1 => 28,
            2 => 26,
            3 => 34,
            4 => 57,
            5 => 63,
            6 => 30,
            7 => 36,
            8 => 38,
            9 => 59,
            _ => 67,
        }
    }

    /// True for formats 6 to 10 (the 30-byte core).
    pub fn is_extended(self) -> bool {
        self.0 >= 6
    }

    /// Byte offset of the GPS time, if the format has one.
    pub fn gps_at(self) -> Option<usize> {
        match self.0 {
            1 | 3 | 4 | 5 => Some(20),
            6..=10 => Some(22),
            _ => None,
        }
    }

    /// Byte offset of the RGB group, if the format has one.
    pub fn rgb_at(self) -> Option<usize> {
        match self.0 {
            2 => Some(20),
            3 | 5 => Some(28),
            7 | 8 | 10 => Some(30),
            _ => None,
        }
    }

    /// Byte offset of the NIR value, if the format has one.
    pub fn nir_at(self) -> Option<usize> {
        match self.0 {
            8 | 10 => Some(36),
            _ => None,
        }
    }

    /// Byte offset of the wave packet group, if the format has one.
    pub fn wave_at(self) -> Option<usize> {
        match self.0 {
            4 => Some(28),
            5 => Some(34),
            9 => Some(30),
            10 => Some(38),
            _ => None,
        }
    }
}
