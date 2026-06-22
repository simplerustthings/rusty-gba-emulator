use super::{BackgroundBuffer, PALETTE_TABLE_BG};
use crate::ppu::{ColorMode, Color15, PIXELS_WIDTH};
use crate::Gba;

#[derive(Copy, Clone)]
struct ScreenEntryRegular(u16);

impl ScreenEntryRegular {
    /// Tile-index of the screen entry.
    #[inline(always)]
    fn tile_index(self) -> u16 {
        self.0 & 0x03FF
    }

    /// Horizontal flip flag.
    #[inline(always)]
    fn hflip(self) -> bool {
        (self.0 & 0x0400) != 0
    }

    /// Vertical flip flag.
    #[inline(always)]
    fn vflip(self) -> bool {
        (self.0 & 0x0800) != 0
    }

    /// Palette bank (in 16 color mode).
    #[inline(always)]
    fn palette_bank(self) -> u32 {
        ((self.0 >> 12) & 0x000F) as u32
    }
}

impl Gba {
    /// Render an affine background in the current scanline.
    pub(super) fn ppu_render_affine_background(
        &mut self,
        index: usize,
        buffer: &mut BackgroundBuffer,
    ) {
        let control = self.ppu.bgcnt[index];
        let (w, h) = control.size.pixels(true);
        let w_i32 = w as i32;
        let h_i32 = h as i32;

        let affine = &self.ppu.bg_affine[index - 2];
        let pa = affine.pa as i32;
        let pc = affine.pc as i32;
        
        // Fast accumulators (eliminates per-pixel multiplication)
        let mut tx_accum = affine.internal_dx;
        let mut ty_accum = affine.internal_dy;

        // Shift bases instead of multiplying
        let entry_address_base = (control.screen_base_block as u32) << 11;
        let tile_address_base_entry = (control.character_base_block as u32) << 14;

        let wm1 = (w as u32) - 1;
        let hm1 = (h as u32) - 1;
        let h_tile = (h as u32) >> 3;

        let vram = &self.ppu.vram;
        let palette = &self.ppu.palette;

        // Hoist the wrap check to the outer scope to avoid branching inner loops
        if control.affine_wrap {
            for pixel in buffer.iter_mut().take(PIXELS_WIDTH) {
                let tx_u32 = ((tx_accum >> 8) as u32) & wm1;
                let ty_u32 = ((ty_accum >> 8) as u32) & hm1;

                tx_accum += pa;
                ty_accum += pc;

                let tile_x = tx_u32 >> 3;
                let tile_y = ty_u32 >> 3;
                let subtile_x = tx_u32 & 7;
                let subtile_y = ty_u32 & 7;

                let entry_offset = tile_x + (tile_y * h_tile);
                let entry_address = entry_address_base + entry_offset;
                let entry = vram[entry_address as usize] as u32;

                let tile_address = tile_address_base_entry + (entry << 6);
                let pixel_offset = (subtile_y * 8 + subtile_x) as usize;
                let data = vram[(tile_address as usize) + pixel_offset];
                if data == 0 {
                    *pixel = Color15::TRANSPARENT;
                } else {
                    let addr = (data as usize) << 1;
                    *pixel = Color15(u16::from_le_bytes([palette[addr], palette[addr + 1]]) & 0x7FFF);
                }
            }
        } else {
            for pixel in buffer.iter_mut().take(PIXELS_WIDTH) {
                let tx = tx_accum >> 8;
                let ty = ty_accum >> 8;

                tx_accum += pa;
                ty_accum += pc;

                if tx >= 0 && tx < w_i32 && ty >= 0 && ty < h_i32 {
                    let tx_u32 = tx as u32;
                    let ty_u32 = ty as u32;

                    let tile_x = tx_u32 >> 3;
                    let tile_y = ty_u32 >> 3;
                    let subtile_x = tx_u32 & 7;
                    let subtile_y = ty_u32 & 7;

                    let entry_offset = tile_x + (tile_y * h_tile);
                    let entry_address = entry_address_base + entry_offset;
                    let entry = vram[entry_address as usize] as u32;

                    let tile_address = tile_address_base_entry + (entry << 6);
                    let pixel_offset = (subtile_y * 8 + subtile_x) as usize;
                    let data = vram[(tile_address as usize) + pixel_offset];
                    if data == 0 {
                        *pixel = Color15::TRANSPARENT;
                    } else {
                        let addr = (data as usize) << 1;
                        *pixel = Color15(u16::from_le_bytes([palette[addr], palette[addr + 1]]) & 0x7FFF);
                    }
                }
            }
        }
    }

    /// Render a regular (non-affine) background in the current scanline.
    pub(super) fn ppu_render_regular_background(
        &mut self,
        index: usize,
        buffer: &mut BackgroundBuffer,
    ) {
        let off_x = (self.ppu.bg_hofs[index] & 0x1FF) as u32;
        let off_y = (self.ppu.bg_vofs[index] & 0x1FF) as u32;
        let control = self.ppu.bgcnt[index];
        let (w, h) = control.size.pixels(false);

        // Bitmasks for powers of 2 (eliminates mod operators)
        let w_mask = (w as u32) - 1;
        let h_mask = (h as u32) - 1;
        let w_is_512 = w == 512;
        let h_is_512 = h == 512;

        let mosaic_enabled = control.mosaic;
        let mut mosaic_x_size = if mosaic_enabled { self.ppu.mosaic.bg_x as u32 } else { 1 };
        if mosaic_x_size == 0 { mosaic_x_size = 1; }

        let screen_y = if mosaic_enabled {
            let size = self.ppu.mosaic.bg_y as u32;
            let sy = self.ppu.vcount as u32;
            if size > 0 { sy - (sy % size) } else { sy }
        } else {
            self.ppu.vcount as u32
        };

        let bg_y = (off_y + screen_y) & h_mask;
        let tile_y = bg_y >> 3;
        let subtile_y = bg_y & 7;

        let entry_address_base = (control.screen_base_block as u32) << 11;
        let tile_address_base = (control.character_base_block as u32) << 14;

        let is_4bpp = matches!(control.color_mode, ColorMode::Bpp4);
        let (tile_row_stride, tile_size) = if is_4bpp { (4u32, 0x20u32) } else { (8u32, 0x40u32) };

        let mut entry_cache = ScreenEntryRegular(0);
        let mut cached_tile_x = u32::MAX;
        let mut tile_base = 0u32;
        let mut palette_base = 0u32;

        let mut eff_screen_x = 0;
        let mut mosaic_counter = 0;

        let subtile_y_normal = subtile_y;
        let subtile_y_flipped = 7 - subtile_y;

        // Borrow VRAM and palette slices once to avoid repeated field access
        let vram = &self.ppu.vram;
        let palette = &self.ppu.palette;

        // Macro-expansion loop hoisting: completely eliminates inner-loop branching for 4bpp/8bpp modes
        macro_rules! render_loop {
            ($is_4bpp_mode:expr) => {
                for original_screen_x in 0..(PIXELS_WIDTH as u32) {
                    
                    // Division-free mosaic simulation
                    if mosaic_enabled {
                        if mosaic_counter == mosaic_x_size {
                            eff_screen_x += mosaic_x_size;
                            mosaic_counter = 0;
                        }
                        mosaic_counter += 1;
                    } else {
                        eff_screen_x = original_screen_x;
                    }

                    let bg_x = (off_x + eff_screen_x) & w_mask;
                    let tile_x = bg_x >> 3;
                    let mut subtile_x = bg_x & 7;

                    if tile_x != cached_tile_x {
                        cached_tile_x = tile_x;

                        let mut idx = tile_x + (tile_y << 5); // << 5 instead of * 32
                        if tile_x >= 32 { idx += 0x03E0; }
                        if w_is_512 && h_is_512 && tile_y >= 32 { idx += 0x0400; }

                        let entry_address = entry_address_base + (idx << 1);
                        let a = entry_address as usize;
                        entry_cache = ScreenEntryRegular(u16::from_le_bytes([vram[a], vram[a + 1]]));

                        let sy = if entry_cache.vflip() { subtile_y_flipped } else { subtile_y_normal };
                        tile_base = tile_address_base + (tile_size * (entry_cache.tile_index() as u32)) + (sy * tile_row_stride);

                        if $is_4bpp_mode {
                            palette_base = PALETTE_TABLE_BG + (entry_cache.palette_bank() << 5);
                        }
                    }

                    if entry_cache.hflip() {
                        subtile_x = 7 - subtile_x;
                    }

                    let color = if $is_4bpp_mode {
                        let byte_offset = (tile_base + (subtile_x >> 1)) as usize;
                        let data = vram[byte_offset];
                        let shift = (subtile_x as u8 & 1) << 2;
                        let index = (data >> shift) & 0xF;

                        if index == 0 {
                            Color15::TRANSPARENT
                        } else {
                            let addr = (palette_base + ((index as u32) << 1)) as usize;
                            Color15(u16::from_le_bytes([palette[addr], palette[addr + 1]]) & 0x7FFF)
                        }
                    } else {
                        let index = vram[(tile_base + subtile_x) as usize];
                        if index == 0 {
                            Color15::TRANSPARENT
                        } else {
                            let addr = (PALETTE_TABLE_BG + ((index as u32) << 1)) as usize;
                            Color15(u16::from_le_bytes([palette[addr], palette[addr + 1]]) & 0x7FFF)
                        }
                    };

                    buffer[original_screen_x as usize] = color;
                }
            }
        }

        if is_4bpp {
            render_loop!(true);
        } else {
            render_loop!(false);
        }
    }
}