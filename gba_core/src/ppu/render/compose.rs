use super::{BackgroundBuffer, ObjectBuffer};
use crate::ppu::{
    color::Color15,
    registers::{BlendMode, WindowControl},
    PIXELS_WIDTH,
};
use crate::Gba;

impl Gba {
    pub(super) fn ppu_compose_scanline(
        &mut self,
        object_buffer: &ObjectBuffer,
        background_buffers: &[BackgroundBuffer; 4],
        background_indices: &mut [usize],
    ) {
        let framebuffer_offset = PIXELS_WIDTH * (self.ppu.vcount as usize);
        let backdrop_color = Color15(u16::from_le_bytes([self.ppu.palette[0], self.ppu.palette[1]]));
        // Precompute the backdrop's ARGB value once per scanline. Pixels that
        // resolve to the backdrop color can reuse this directly, skipping the
        // ~4-shift-bundle of `Color15::as_argb()` per such pixel. Bit-identical
        // output: any color equal to the backdrop produces the same ARGB.
        let backdrop_argb = backdrop_color.as_argb();

        background_indices.sort_by_key(|&x| self.ppu.bgcnt[x].priority);
        let priorities: [u16; 4] = {
            let mut p = [0u16; 4];
            for &i in background_indices.iter() {
                p[i] = self.ppu.bgcnt[i].priority;
            }
            p
        };

        let dispcnt = &self.ppu.dispcnt;
        let windows_enabled = dispcnt.windows_enabled();
        let obj_window_display = dispcnt.obj_window_display;
        let display_obj = dispcnt.display_obj;
        let win_display = dispcnt.window_display;
        let win_in = &self.ppu.win_in;
        let win_out = &self.ppu.win_out;
        let win_h = &self.ppu.win_h;
        let window_scanline_active = &self.ppu.window_scanline_active;
        let bldcnt = &self.ppu.bldcnt;
        let bldalpha = &self.ppu.bldalpha;
        let bldy = &self.ppu.bldy;
        let fb = &mut self.ppu.framebuffer;

        // Precompute blend lookup: for each top-kind, whether blending is possible
        let blend_top = &bldcnt.top;
        let blend_bottom = &bldcnt.bottom;
        let blend_mode = bldcnt.mode;

        if !windows_enabled {
            // Fast path: no windows enabled. All layers and blend active.
            let no_win = WindowControl::none();
            let backdrop_layer = Layer::backdrop(backdrop_color);
            for x in 0..PIXELS_WIDTH {
                let obj = &object_buffer[x];

                // Walk the priority-sorted bg list once and pick the first two
                // non-transparent, window-eligible backgrounds. Replaces the
                // per-pixel `Iter::filter` state machine with a tight manual loop
                // over (at most 4) entries.
                let mut top = backdrop_layer;
                let mut bottom = backdrop_layer;
                let mut found_top = false;
                for &i in background_indices.iter() {
                    if !background_buffers[i][x].transparent() && no_win.layer[i] {
                        if !found_top {
                            top = Layer::background(i, background_buffers[i][x], priorities[i]);
                            found_top = true;
                        } else {
                            bottom = Layer::background(i, background_buffers[i][x], priorities[i]);
                            break;
                        }
                    }
                }

                if display_obj && !obj.color.transparent() && no_win.layer[KIND_OBJ] {
                    if obj.priority <= top.priority {
                        bottom = top;
                        top = Layer::object(obj.color, obj.priority);
                    } else if obj.priority <= bottom.priority {
                        bottom = Layer::object(obj.color, obj.priority);
                    }
                }

                let color = if !(no_win.blend || obj.blend) {
                    top.color
                } else {
                    let b_top = blend_top[top.kind];
                    let b_bottom = blend_bottom[bottom.kind];
                    let b_mode = if obj.blend && b_bottom {
                        BlendMode::Normal
                    } else {
                        blend_mode
                    };
                    let blend_enabled = (b_top && b_mode != BlendMode::None) || obj.blend;

                    if blend_enabled {
                        match b_mode {
                            BlendMode::Normal if b_bottom => Color15::blend(
                                top.color,
                                bottom.color,
                                bldalpha.top,
                                bldalpha.bottom,
                            ),
                            BlendMode::White => {
                                let fade = bldy.fade.min(16);
                                Color15::blend(top.color, Color15::WHITE, 16 - fade, fade)
                            }
                            BlendMode::Black => {
                                let fade = bldy.fade.min(16);
                                Color15::blend(top.color, Color15::BLACK, 16 - fade, fade)
                            }
                            _ => top.color,
                        }
                    } else {
                        top.color
                    }
                };
                fb[framebuffer_offset + x] = if color.0 == backdrop_color.0 { backdrop_argb } else { color.as_argb() };
            }
        } else {
            let backdrop_layer = Layer::backdrop(backdrop_color);
            for x in 0..PIXELS_WIDTH {
                let obj = &object_buffer[x];

                let window = if win_display[0]
                    && window_scanline_active[0]
                    && win_h[0].test(x)
                {
                    win_in.win0
                } else if win_display[1]
                    && window_scanline_active[1]
                    && win_h[1].test(x)
                {
                    win_in.win1
                } else if obj_window_display && obj.window {
                    win_out.win_obj
                } else {
                    win_out.win_out
                };

                // Manual walk; see comment in the no-windows branch.
                let mut top = backdrop_layer;
                let mut bottom = backdrop_layer;
                let mut found_top = false;
                for &i in background_indices.iter() {
                    if !background_buffers[i][x].transparent() && window.layer[i] {
                        if !found_top {
                            top = Layer::background(i, background_buffers[i][x], priorities[i]);
                            found_top = true;
                        } else {
                            bottom = Layer::background(i, background_buffers[i][x], priorities[i]);
                            break;
                        }
                    }
                }

                if display_obj && !obj.color.transparent() && window.layer[KIND_OBJ] {
                    if obj.priority <= top.priority {
                        bottom = top;
                        top = Layer::object(obj.color, obj.priority);
                    } else if obj.priority <= bottom.priority {
                        bottom = Layer::object(obj.color, obj.priority);
                    }
                }

                let color = if !(window.blend || obj.blend) {
                    top.color
                } else {
                    let b_top = blend_top[top.kind];
                    let b_bottom = blend_bottom[bottom.kind];
                    let b_mode = if obj.blend && b_bottom {
                        BlendMode::Normal
                    } else {
                        blend_mode
                    };
                    let blend_enabled = (b_top && b_mode != BlendMode::None) || obj.blend;

                    if blend_enabled {
                        match b_mode {
                            BlendMode::Normal if b_bottom => Color15::blend(
                                top.color,
                                bottom.color,
                                bldalpha.top,
                                bldalpha.bottom,
                            ),
                            BlendMode::White => {
                                let fade = bldy.fade.min(16);
                                Color15::blend(top.color, Color15::WHITE, 16 - fade, fade)
                            }
                            BlendMode::Black => {
                                let fade = bldy.fade.min(16);
                                Color15::blend(top.color, Color15::BLACK, 16 - fade, fade)
                            }
                            _ => top.color,
                        }
                    } else {
                        top.color
                    }
                };
                fb[framebuffer_offset + x] = if color.0 == backdrop_color.0 { backdrop_argb } else { color.as_argb() };
            }
        }
    }
}

#[derive(Copy, Clone)]
struct Layer {
    kind: usize,
    color: Color15,
    priority: u16,
}

impl Layer {
    #[inline(always)]
    fn backdrop(color: Color15) -> Layer {
        Layer {
            kind: KIND_BACKDROP,
            color,
            priority: u16::MAX,
        }
    }

    #[inline(always)]
    fn background(index: usize, color: Color15, priority: u16) -> Layer {
        Layer {
            kind: index,
            color,
            priority,
        }
    }

    #[inline(always)]
    fn object(color: Color15, priority: u16) -> Layer {
        Layer {
            kind: KIND_OBJ,
            color,
            priority,
        }
    }
}

const KIND_OBJ: usize = 4;
const KIND_BACKDROP: usize = 5;
