// Synthetic fixture (CC-BY-4.0) for UX.SYSTEM.LINT AC-2.
pub struct Color32(pub u8, pub u8, pub u8);

impl Color32 {
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }
}

pub fn accent_color() -> Color32 {
    // Hard-coded colour literal on line 12:
    Color32::from_rgb(0x1f, 0x6f, 0xeb)
}
