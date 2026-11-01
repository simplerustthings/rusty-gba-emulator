use crate::{
    scheduler::{Event, PpuEvent},
    Gba, InterruptKind, HEIGHT, WIDTH,
};
use color::Color15;
use registers::*;

mod color;
mod registers;
mod render;

#[allow(unused)]
mod constants {
    pub const PIXELS_WIDTH: usize = crate::WIDTH;
    pub const PIXELS_HEIGHT: usize = crate::HEIGHT;
    pub const PIXELS_HBLANK: usize = 68;
    pub const SCANLINES_VBLANK: usize = 68;
    pub const CYCLES_PIXEL: usize = 4;
    pub const CYCLES_HDRAW: usize = PIXELS_WIDTH * CYCLES_PIXEL;
    pub const CYCLES_HBLANK: usize = PIXELS_HBLANK * CYCLES_PIXEL;
    pub const CYCLES_SCANLINE: usize = CYCLES_HDRAW + CYCLES_HBLANK;
    pub const CYCLES_VDRAW: usize = CYCLES_SCANLINE * PIXELS_HEIGHT;
    pub const CYCLES_VBLANK: usize = CYCLES_SCANLINE * SCANLINES_VBLANK;
    pub const CYCLES_FRAME: usize = CYCLES_VDRAW + CYCLES_VBLANK;
}
pub use constants::*;
use crate::{impl_save_state_fields, impl_save_state_enum};

pub struct Ppu {
    /// Framebuffer: row major, each pixel is ARGB, length (WIDTH * HEIGHT).
    pub framebuffer: Box<[u32]>,

    /// Register DISPCNT - LCD Control
    pub dispcnt: DisplayControl,

    /// Register DISPSTAT - General LCD Status,
    pub dispstat: DisplayStatus,

    /// Registers BGxCNT - Background Control
    pub bgcnt: [BackgroundControl; 4],

    /// Registers BGxHOFS - Background X-Offsets
    pub bg_hofs: [u16; 4],

    /// Registers BGxVOFS - Background Y-Offsets
    pub bg_vofs: [u16; 4],

    /// Background Affine Registers.
    pub bg_affine: [BackgroundAffine; 2],

    /// Window horizontal bounds registers.
    pub win_h: [WindowBounds; 2],

    /// Window vertical bounds registers.
    pub win_v: [WindowBounds; 2],

    /// Register WININ - Control for Window 0 and 1.
    pub win_in: WindowIn,

    /// Register WINOUT - Control for Window Obj and Out.
    pub win_out: WindowOut,

    /// Register MOSAIC - Mosaic Size.
    pub mosaic: Mosaic,

    /// Register BLDCNT - Blend Control.
    pub bldcnt: BlendControl,

    /// Register BLDALPHA - Blend Alpha.
    pub bldalpha: BlendAlpha,

    /// Register BLDY - Blend Fade.
    pub bldy: BlendFade,

    /// Current scanline (0..=227). 160..=227 are in vblank.
    pub vcount: u16,

    /// VRAM - Video Ram - 96 KiB
    pub vram: Box<[u8]>,

    /// BG/OBJ Palette RAM - 1 KiB
    pub palette: Box<[u8]>,

    /// OAM - Object Attribute Memory - 1 KiB
    pub oam: Box<[u8]>,

    /// Whether the rectangle windows are enabled for the current scanline.
    /// This is updated on each scanline.
    pub window_scanline_active: [bool; 2],

    /// Current frame.
    #[allow(unused)]
    pub frame: usize,
}

impl_save_state_fields!(Ppu {
    framebuffer,
    dispcnt,
    dispstat,
    bgcnt,
    bg_hofs,
    bg_vofs,
    bg_affine,
    win_h,
    win_v,
    win_in,
    win_out,
    mosaic,
    bldcnt,
    bldalpha,
    bldy,
    vcount,
    vram,
    palette,
    oam,
    window_scanline_active,
    frame,
});

impl Ppu {
    pub fn new() -> Ppu {
        Ppu {
            framebuffer: vec![0; WIDTH * HEIGHT].into_boxed_slice(),
            dispcnt: DisplayControl::default(),
            dispstat: DisplayStatus::default(),
            bgcnt: <[BackgroundControl; 4]>::default(),
            bg_hofs: [0; 4],
            bg_vofs: [0; 4],
            bg_affine: <[BackgroundAffine; 2]>::default(),
            win_h: [WindowBounds::default(); 2],
            win_v: [WindowBounds::default(); 2],
            win_in: WindowIn::default(),
            win_out: WindowOut::default(),
            mosaic: Mosaic::default(),
            bldcnt: BlendControl::default(),
            bldalpha: BlendAlpha::default(),
            bldy: BlendFade::default(),
            vcount: 0,
            frame: 0,
            window_scanline_active: [false; 2],

            // 96KiB, but we'll make it 128KiB for accesses
            vram: vec![0; 128 * 1024].into_boxed_slice(),
            palette: vec![0; 1024].into_boxed_slice(),
            oam: vec![0; 1024].into_boxed_slice(),
        }
    }

    pub fn skip_bios(&mut self) {
        self.bg_affine[0].pa = 0x100;
        self.bg_affine[0].pb = 0;
        self.bg_affine[0].pc = 0;
        self.bg_affine[0].pd = 0x100;
        self.bg_affine[1].pa = 0x100;
        self.bg_affine[1].pb = 0;
        self.bg_affine[1].pc = 0;
        self.bg_affine[1].pd = 0x100;
    }
}

impl Gba {
    pub fn ppu_init(&mut self) {
        self.scheduler
            .push_event(Event::Ppu(PpuEvent::EndHDraw), CYCLES_HDRAW);
    }

    fn update_vcount(&mut self, new_vcount: u16) {
        self.ppu.vcount = new_vcount;
        self.ppu.dispstat.vcounter = self.ppu.dispstat.vcount_setting == new_vcount;
        if self.ppu.dispstat.vcounter_irq && self.ppu.dispstat.vcounter {
            self.interrupt_raise(InterruptKind::VCount);
        }

        if new_vcount as u8 == self.ppu.win_v[0].min {
            self.ppu.window_scanline_active[0] = true;
        }
        if new_vcount as u8 == self.ppu.win_v[0].max {
            self.ppu.window_scanline_active[0] = false;
        }
        if new_vcount as u8 == self.ppu.win_v[1].min {
            self.ppu.window_scanline_active[1] = true;
        }
        if new_vcount as u8 == self.ppu.win_v[1].max {
            self.ppu.window_scanline_active[1] = false;
        }
    }

    pub fn ppu_on_event(&mut self, event: PpuEvent, lateness: usize) {
        let (next_event, deadline) = match event {
            PpuEvent::EndHDraw => self.ppu_on_end_hdraw(),
            PpuEvent::EndHBlank => self.ppu_on_end_hblank(),
            PpuEvent::EndVBlankHDraw => self.ppu_on_end_vblank_hdraw(),
            PpuEvent::EndVBlankHBlank => self.ppu_on_end_vblank_hblank(),
        };
        let deadline = deadline - lateness;
        self.scheduler.push_event(Event::Ppu(next_event), deadline);
    }

    fn ppu_on_end_hdraw(&mut self) -> (PpuEvent, usize) {
        self.ppu.dispstat.hblank = true;
        if self.ppu.dispstat.hblank_irq {
            self.interrupt_raise(InterruptKind::HBlank);
        }

        self.dma_notify_hblank();
        if self.ppu.vcount >= 2 {
            self.dma_notify_video();
        }

        (PpuEvent::EndHBlank, CYCLES_HBLANK)
    }

    fn ppu_on_end_hblank(&mut self) -> (PpuEvent, usize) {
        // Increment the scanline.
        self.ppu.dispstat.hblank = false;
        self.update_vcount(self.ppu.vcount + 1);

        if (self.ppu.vcount as usize) == PIXELS_HEIGHT {
            // Just entered vblank.
            self.ppu.dispstat.vblank = true;
            if self.ppu.dispstat.vblank_irq {
                self.interrupt_raise(InterruptKind::VBlank);
            }
            self.dma_notify_vblank();

            self.ppu.bg_affine[0].internal_dx = self.ppu.bg_affine[0].dx;
            self.ppu.bg_affine[0].internal_dy = self.ppu.bg_affine[0].dy;
            self.ppu.bg_affine[1].internal_dx = self.ppu.bg_affine[1].dx;
            self.ppu.bg_affine[1].internal_dy = self.ppu.bg_affine[1].dy;

            (PpuEvent::EndVBlankHDraw, CYCLES_HDRAW)
        } else {
            self.ppu.bg_affine[0].internal_dx += self.ppu.bg_affine[0].pb as i32;
            self.ppu.bg_affine[0].internal_dy += self.ppu.bg_affine[0].pd as i32;
            self.ppu.bg_affine[1].internal_dx += self.ppu.bg_affine[1].pb as i32;
            self.ppu.bg_affine[1].internal_dy += self.ppu.bg_affine[1].pd as i32;

            // Draw the next scanline (which is visible).
            if self.should_render {
                self.ppu_render_scanline();
            }

            (PpuEvent::EndHDraw, CYCLES_HDRAW)
        }
    }

    fn ppu_on_end_vblank_hdraw(&mut self) -> (PpuEvent, usize) {
        self.ppu.dispstat.hblank = true;
        if self.ppu.dispstat.hblank_irq {
            self.interrupt_raise(InterruptKind::HBlank);
        }
        if self.ppu.vcount < 162 {
            self.dma_notify_video();
        }

        (PpuEvent::EndVBlankHBlank, CYCLES_HBLANK)
    }

    fn ppu_on_end_vblank_hblank(&mut self) -> (PpuEvent, usize) {
        // Increment the scanline.
        self.ppu.dispstat.hblank = false;
        let new_vcount = self.ppu.vcount + 1;

        if (new_vcount as usize) == (PIXELS_HEIGHT + SCANLINES_VBLANK) {
            // Finished vblank.
            self.ppu.dispstat.vblank = false;
            self.update_vcount(0);
            self.ppu.frame += 1;

            // Draw the first scanline.
            self.ppu_render_scanline();

            (PpuEvent::EndHDraw, CYCLES_HDRAW)
        } else {
            // Another vblank scanline.
            self.update_vcount(new_vcount);
            (PpuEvent::EndVBlankHDraw, CYCLES_HDRAW)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorMode {
    /// 4 bits per pixel (16 colors).
    Bpp4 = 0,
    /// 8 bits per pixel (256 colors).
    Bpp8 = 1,
}

impl_save_state_enum!(ColorMode { Bpp4, Bpp8 });
