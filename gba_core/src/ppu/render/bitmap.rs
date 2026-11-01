use super::super::{constants::*, Color15};
use super::{BackgroundBuffer, PALETTE_TABLE_BG};
use crate::Gba;

impl Gba {
    /// Render bitmap mode 3: 240x160, 16 bpp
    pub(super) fn ppu_render_bitmap_3(&mut self, buffer: &mut BackgroundBuffer) {
        let (w, h) = (240, 160);
        let vram = &self.ppu.vram;
        for (screen_x, pixel) in buffer.iter_mut().enumerate().take(PIXELS_WIDTH) {
            if let Some((tx, ty)) = self.bg_affine_transform(2, screen_x as i32, w, h) {
                let address = (2 * ((w as u32) * ty + tx)) as usize;
                let color = Color15(u16::from_le_bytes([vram[address], vram[address + 1]]));
                *pixel = color;
            }
        }
    }

    /// Render bitmap mode 4: Bitmap: 240x160, 8 bpp (palette) (allows page flipping)
    pub(super) fn ppu_render_bitmap_4(&mut self, buffer: &mut BackgroundBuffer) {
        let (w, h) = (240, 160);
        let page_address = 0xA000 * (self.ppu.dispcnt.display_frame as usize);
        let vram = &self.ppu.vram;
        let palette = &self.ppu.palette;

        for (screen_x, pixel) in buffer.iter_mut().enumerate().take(PIXELS_WIDTH) {
            if let Some((tx, ty)) = self.bg_affine_transform(2, screen_x as i32, w, h) {
                let address = page_address + (((w as u32) * ty + tx) as usize);
                let index = vram[address];
                let addr = (PALETTE_TABLE_BG + ((index as u32) << 1)) as usize;
                let color = Color15(u16::from_le_bytes([palette[addr], palette[addr + 1]]) & 0x7FFF);
                *pixel = color;
            }
        }
    }

    /// Render bitmap mode 5: 160x128 pixels, 16 bpp, allows page flipping
    pub(super) fn ppu_render_bitmap_5(&mut self, buffer: &mut BackgroundBuffer) {
        let (w, h) = (160, 128);
        let page_address = 0xA000 * (self.ppu.dispcnt.display_frame as u32);
        let vram = &self.ppu.vram;

        for (screen_x, pixel) in buffer.iter_mut().enumerate().take(PIXELS_WIDTH) {
            if let Some((tx, ty)) = self.bg_affine_transform(2, screen_x as i32, w, h) {
                let offset = (2 * ((w as u32) * ty + tx)) as usize;
                let address = (page_address as usize) + offset;
                let color = Color15(u16::from_le_bytes([vram[address], vram[address + 1]]));
                *pixel = color;
            }
        }
    }
}
