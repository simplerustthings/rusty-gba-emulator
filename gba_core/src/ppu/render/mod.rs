use super::constants::*;
use super::Color15;
use crate::Gba;

mod backgrounds;
mod bitmap;
mod compose;
mod objects;

const PALETTE_TABLE_BG: u32 = 0x0000;
const PALETTE_TABLE_OBJ: u32 = 0x0200;

/// Affine transformation matrix.
struct AffineMatrix {
    pa: i32,
    pb: i32,
    pc: i32,
    pd: i32,
}

/// Entry in the scanline object buffer.
/// Used to keep track of objects and priorities as we're rendering a scanline.
#[derive(Copy, Clone)]
struct ObjectBufferEntry {
    pub color: Color15,
    pub priority: u16,
    pub blend: bool,
    pub window: bool,
}

impl ObjectBufferEntry {
    #[inline(always)]
    fn set(&mut self, color: Color15, attributes: &objects::ObjectAttributes) {
        if attributes.window() {
            self.window = true;
        } else {
            let priority = attributes.priority();
            if priority < self.priority {
                self.color = color;
                self.priority = priority;
                self.blend = attributes.gfx_mode() == objects::GraphicsMode::Blend;
            }
        }
    }
}

impl Default for ObjectBufferEntry {
    fn default() -> Self {
        Self {
            color: Color15::TRANSPARENT,
            priority: u16::MAX,
            blend: false,
            window: false,
        }
    }
}

/// Object scanline buffer.
type ObjectBuffer = [ObjectBufferEntry; PIXELS_WIDTH];

/// Background scanline buffer.
type BackgroundBuffer = [Color15; PIXELS_WIDTH];

impl Gba {
    /// Render the current scanline.
    pub(super) fn ppu_render_scanline(&mut self) {
        // Forced blank shows all white.
        if self.ppu.dispcnt.forced_blank {
            let framebuffer_offset = PIXELS_WIDTH * (self.ppu.vcount as usize);
            let white = Color15::WHITE.as_argb();
            let fb = &mut self.ppu.framebuffer;
            fb[framebuffer_offset..framebuffer_offset + PIXELS_WIDTH].fill(white);
            return;
        }

        // Render objects.
        let mut object_buffer = [ObjectBufferEntry::default(); PIXELS_WIDTH];
        if self.ppu.dispcnt.display_obj {
            self.ppu_render_objects(&mut object_buffer);
        }

        // Render backgrounds.
        let mut background_buffers = [[Color15::TRANSPARENT; PIXELS_WIDTH]; 4];
        let mut background_indices = [0usize; 4];
        let mut background_count = 0;
        match self.ppu.dispcnt.mode {
            0 => {
                if self.ppu.dispcnt.display_bg[0] {
                    self.ppu_render_regular_background(0, &mut background_buffers[0]);
                    background_indices[background_count] = 0;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[1] {
                    self.ppu_render_regular_background(1, &mut background_buffers[1]);
                    background_indices[background_count] = 1;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[2] {
                    self.ppu_render_regular_background(2, &mut background_buffers[2]);
                    background_indices[background_count] = 2;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[3] {
                    self.ppu_render_regular_background(3, &mut background_buffers[3]);
                    background_indices[background_count] = 3;
                    background_count += 1;
                }
            }
            1 => {
                if self.ppu.dispcnt.display_bg[0] {
                    self.ppu_render_regular_background(0, &mut background_buffers[0]);
                    background_indices[background_count] = 0;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[1] {
                    self.ppu_render_regular_background(1, &mut background_buffers[1]);
                    background_indices[background_count] = 1;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[2] {
                    self.ppu_render_affine_background(2, &mut background_buffers[2]);
                    background_indices[background_count] = 2;
                    background_count += 1;
                }
            }
            2 => {
                if self.ppu.dispcnt.display_bg[2] {
                    self.ppu_render_affine_background(2, &mut background_buffers[2]);
                    background_indices[background_count] = 2;
                    background_count += 1;
                }
                if self.ppu.dispcnt.display_bg[3] {
                    self.ppu_render_affine_background(3, &mut background_buffers[3]);
                    background_indices[background_count] = 3;
                    background_count += 1;
                }
            }
            3 => {
                // Mode 3: Bitmap: 240x160, 16 bpp
                if self.ppu.dispcnt.display_bg[2] {
                    let buffer = &mut background_buffers[2];
                    self.ppu_render_bitmap_3(buffer);
                    background_indices[0] = 2;
                    background_count = 1;
                }
            }
            4 => {
                // Mode 4: Bitmap: 240x160, 8 bpp (palette) (allows page flipping)
                if self.ppu.dispcnt.display_bg[2] {
                    let buffer = &mut background_buffers[2];
                    self.ppu_render_bitmap_4(buffer);
                    background_indices[0] = 2;
                    background_count = 1;
                }
            }
            5 => {
                // Mode 5: Bitmap: 160x128 pixels, 16 bpp, allows page flipping
                if self.ppu.dispcnt.display_bg[2] {
                    let buffer = &mut background_buffers[2];
                    self.ppu_render_bitmap_5(buffer);
                    background_indices[0] = 2;
                    background_count = 1;
                }
            }
            m => panic!("Unsupported video mode {}", m),
        }

        self.ppu_compose_scanline(
            &object_buffer,
            &background_buffers,
            &mut background_indices[..background_count],
        );
    }

    /// Get a palette index from a 4bpp tile.
    ///
    /// `address`: the address of the tile in VRAM
    /// `x`: the x coordinate of the pixel in the tile
    /// `y`: the y coordinate of the pixel in the tile
    #[inline(always)]
    fn tile_4bpp_get_index(&self, address: u32, x: u32, y: u32) -> u8 {
        let pixel = y * 8 + x;
        let address = (address + (pixel >> 1)) as usize;
        let data = self.ppu.vram[address];
        (data >> ((pixel & 1) << 2)) & 0xF
    }

    /// Get a palette index from an 8bpp tile.
    ///
    /// `address`: the address of the tile in VRAM
    /// `x`: the x coordinate of the pixel in the tile
    /// `y`: the y coordinate of the pixel in the tile
    #[inline(always)]
    fn tile_8bpp_get_index(&self, address: u32, x: u32, y: u32) -> u8 {
        let pixel = y * 8 + x;
        let address = (address + pixel) as usize;
        self.ppu.vram[address]
    }

    /// Get a color from a palette using direct byte reads.
    ///
    /// `index`: the index of the color in the palette
    /// `bank`: the palette bank
    /// `table`: selects between sprite and bg palettes
    #[inline(always)]
    fn palette_get_color(&self, index: u8, bank: u32, table: u32) -> Color15 {
        if index == 0 {
            Color15::TRANSPARENT
        } else {
            let address = (table + (2 * index as u32) + (32 * bank)) as usize;
            let raw = u16::from_le_bytes([self.ppu.palette[address], self.ppu.palette[address + 1]]);
            Color15(raw & 0x7FFF)
        }
    }

    /// Do the affine background transformation for the given background
    /// for the current scanline and given screen x position.
    ///
    /// Returns the texture coordinate, or None if it's out of bounds.
    #[inline(always)]
    fn bg_affine_transform(
        &self,
        index: usize,
        screen_x: i32,
        w: i32,
        h: i32,
    ) -> Option<(u32, u32)> {
        let affine = self.ppu.bg_affine[index - 2];
        let (dx, dy) = (affine.internal_dx, affine.internal_dy);

        let tx = (dx + screen_x * (affine.pa as i32)) >> 8;
        let ty = (dy + screen_x * (affine.pc as i32)) >> 8;
        if tx < 0 || tx >= w || ty < 0 || ty >= h {
            if self.ppu.bgcnt[index].affine_wrap {
                Some(((tx as u32) & (w as u32 - 1), (ty as u32) & (h as u32 - 1)))
            } else {
                None
            }
        } else {
            Some((tx as u32, ty as u32))
        }
    }
}
