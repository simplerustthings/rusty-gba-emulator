#[derive(Copy, Clone, PartialEq, Eq)]
pub struct Color15(pub u16);

impl Color15 {
    pub const TRANSPARENT: Color15 = Color15(0x8000);
    pub const WHITE: Color15 = Color15(0x7FFF);
    pub const BLACK: Color15 = Color15(0x0000);

    /// Convert the 15-bit color to 32-bit ARGB.
    #[inline(always)]
    pub fn as_argb(self) -> u32 {
        let c = self.0 as u32;
        0xFF00_0000 | ((c & 0x001F) << 19) | ((c & 0x03E0) << 6) | ((c & 0x7C00) >> 7)
    }

    #[allow(dead_code)]
    pub fn as_rgb(self) -> (u16, u16, u16) {
        let r = self.0 & 0b11111;
        let g = (self.0 >> 5) & 0b11111;
        let b = (self.0 >> 10) & 0b11111;
        (r, g, b)
    }

    /// Convert three components to a color. r, g, b must all be < 32.
    #[allow(dead_code)]
    pub fn from_rgb(r: u16, g: u16, b: u16) -> Color15 {
        Color15(r | (g << 5) | (b << 10))
    }

    #[inline(always)]
    pub fn transparent(self) -> bool {
        self == Color15::TRANSPARENT
    }

    /// Blend two 15-bit colors with weighted average.
    /// Operates directly on packed channels to avoid decompose/recompose overhead.
    #[inline(always)]
    pub fn blend(a: Color15, b: Color15, a_weight: u16, b_weight: u16) -> Color15 {
        let aw = a_weight.min(16) as u32;
        let bw = b_weight.min(16) as u32;
        let a = a.0 as u32;
        let bv = b.0 as u32;
        let r = ((a & 0x1F) * aw + (bv & 0x1F) * bw) >> 4;
        let g = (((a >> 5) & 0x1F) * aw + ((bv >> 5) & 0x1F) * bw) >> 4;
        let bl = (((a >> 10) & 0x1F) * aw + ((bv >> 10) & 0x1F) * bw) >> 4;
        Color15((r.min(31) | (g.min(31) << 5) | (bl.min(31) << 10)) as u16)
    }
}
