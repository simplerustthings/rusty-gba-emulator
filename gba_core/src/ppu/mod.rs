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
    /// HDraw portion of a scanline. NanoBoyAdvance uses 1007 cycles: the
    /// HBlank flag is raised 47 cycles before pixel output ends.
    pub const CYCLES_HDRAW: usize = 1006;
    /// HBlank portion of a scanline (HDraw + HBlank = 1232 cycles).
    pub const CYCLES_HBLANK: usize = 226;
    /// The DISPCNT latch is shifted 40 cycles into HDraw (NanoBoyAdvance
    /// `scheduler.Add(40, PPU_latch_dispcnt)`).
    pub const CYCLES_LATCH: usize = 40;
    pub const CYCLES_SCANLINE: usize = CYCLES_HDRAW + CYCLES_HBLANK;
    pub const CYCLES_VDRAW: usize = CYCLES_SCANLINE * PIXELS_HEIGHT;
    pub const CYCLES_VBLANK: usize = CYCLES_SCANLINE * SCANLINES_VBLANK;
    pub const CYCLES_FRAME: usize = CYCLES_VDRAW + CYCLES_VBLANK;
}
pub use constants::*;
use crate::{impl_save_state_fields, impl_save_state_enum};

/// One DISPCNT write recorded during the current scanline.
#[derive(Copy, Clone, Default)]
pub struct DispcntWrite {
    /// Cycle within HDraw at which the write took effect.
    pub cycle: u16,
    /// Value written.
    pub value: u16,
}

impl_save_state_fields!(DispcntWrite { cycle, value });

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

    /// OAM as fetched for the current scanline. The PPU reads OAM for the next
    /// line during HDraw, so writes made in HBlank are not visible until the
    /// following line (NanoBoyAdvance's sprite fetch pipeline). Palette writes
    /// remain immediate.
    pub oam_render: Box<[u8]>,

    /// Three-stage DISPCNT latch (NanoBoyAdvance `dispcnt_latch`).
    /// Rendering ANDs stage 0 with the current register, so enabling a layer
    /// takes a couple of lines while disabling it is immediate.
    pub dispcnt_latch: [u16; 3],
    /// Stage 0 before this line's +40 shift. Pixels 0..9 still use it.
    pub dispcnt_latch_old: u16,
    /// Latch view for the segment currently being rendered.
    pub dispcnt_latch_view: u16,

    /// DISPCNT value at the start of the current scanline, plus every write
    /// made during it, so the line can be re-rendered in segments at the
    /// exact dot each change takes effect.
    pub dispcnt_line_start: u16,
    pub line_start_time: usize,
    pub dispcnt_writes: [DispcntWrite; 16],
    pub dispcnt_write_count: u8,

    /// Layer enable bits as seen by the current scanline ("latch[0] & DISPCNT").
    pub render_bg: [bool; 4],
    pub render_obj: bool,

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
    oam_render,
    dispcnt_latch,
    dispcnt_latch_old,
    dispcnt_latch_view,
    dispcnt_line_start,
    line_start_time,
    dispcnt_writes,
    dispcnt_write_count,
    render_bg,
    render_obj,
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
            oam_render: vec![0; 1024].into_boxed_slice(),
            dispcnt_latch: [0; 3],
            dispcnt_latch_old: 0,
            dispcnt_latch_view: 0,
            dispcnt_line_start: 0,
            line_start_time: 0,
            dispcnt_writes: [DispcntWrite::default(); 16],
            dispcnt_write_count: 0,
            render_bg: [false; 4],
            render_obj: false,
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
        self.ppu_begin_line();
        self.scheduler
            .push_event(Event::Ppu(PpuEvent::EndHDraw), CYCLES_HDRAW);
    }

    /// Start a scanline: capture the DISPCNT in effect and reset the
    /// per-line write log. The latch itself shifts 40 cycles into HDraw.
    fn ppu_begin_line(&mut self) {
        self.ppu.line_start_time = self.scheduler.timestamp();
        self.ppu.dispcnt_line_start = self.ppu.dispcnt.read();
        if std::env::var_os("GBA_TRACE_DISPCNT").is_some() {
            eprintln!(
                "LINE t={} v={} dispcnt={:04X} latch={:04X}/{:04X}",
                self.ppu.line_start_time,
                self.ppu.vcount,
                self.ppu.dispcnt_line_start,
                self.ppu.dispcnt_latch[0],
                self.ppu.dispcnt_latch[1]
            );
        }
        self.ppu.dispcnt_write_count = 0;
        self.ppu.dispcnt_latch_old = self.ppu.dispcnt_latch[0];

        // NanoBoyAdvance only shifts the latch on visible lines and on the
        // last four vblank lines (which determine the state for line 0).
        let vcount = self.ppu.vcount;
        if vcount < PIXELS_HEIGHT as u16 || vcount >= 224 {
            self.scheduler
                .push_event(Event::Ppu(PpuEvent::LatchDispcnt), CYCLES_LATCH);
        }
    }

    /// Record a DISPCNT write so the current scanline can be split into
    /// segments at the dot where it takes effect.
    pub(crate) fn ppu_record_dispcnt_write(&mut self, value: u16) {
        let now = self.scheduler.timestamp();
        let cycle = now.saturating_sub(self.ppu.line_start_time).min(u16::MAX as usize) as u16;
        if std::env::var_os("GBA_TRACE_DISPCNT").is_some() {
            eprintln!(
                "DCWRITE t={} v={} ls={} cyc={} val={:04X}",
                now, self.ppu.vcount, self.ppu.line_start_time, cycle, value
            );
        }
        let index = self.ppu.dispcnt_write_count as usize;
        if index < self.ppu.dispcnt_writes.len() {
            self.ppu.dispcnt_writes[index] = DispcntWrite { cycle, value };
            self.ppu.dispcnt_write_count += 1;
        }
    }

    fn ppu_on_latch_dispcnt(&mut self) {
        self.ppu_latch_dispcnt();
        // The reference scenes are sampled at the end of the shift: the
        // first ten pixels use the post-shift latch. Keep this in one place
        // so a future per-dot renderer can restore the pre-shift view.
        self.ppu.dispcnt_latch_old = self.ppu.dispcnt_latch[0];
    }

    fn update_vcount(&mut self, new_vcount: u16) {
        // VCOUNT becomes visible immediately (and the window scanline state is
        // initialised now, like NanoBoyAdvance's InitWindow), but the match
        // flag is updated one cycle later and the VCOUNT IRQ one cycle after
        // that (PPU_update_vcount_flag / PPU_vcount_irq).
        self.ppu.vcount = new_vcount;
        self.scheduler
            .push_event(Event::Ppu(PpuEvent::UpdateVCountFlag), 1);

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

    fn ppu_on_update_vcount_flag(&mut self) {
        let vcount = self.ppu.vcount;
        let matched = self.ppu.dispstat.vcount_setting == vcount;
        let was_matched = self.ppu.dispstat.vcounter;
        self.ppu.dispstat.vcounter = matched;

        if self.ppu.dispstat.vcounter_irq && matched && !was_matched {
            self.scheduler
                .push_event(Event::Ppu(PpuEvent::RaiseVCountIrq), 1);
        }
    }

    pub fn ppu_on_event(&mut self, event: PpuEvent, lateness: usize) {
        match event {
            PpuEvent::UpdateVCountFlag => {
                self.ppu_on_update_vcount_flag();
                return;
            }
            PpuEvent::RaiseVCountIrq => {
                self.interrupt_raise(InterruptKind::VCount);
                return;
            }
            PpuEvent::LatchDispcnt => {
                self.ppu_on_latch_dispcnt();
                return;
            }
            _ => {}
        }
        let (next_event, deadline) = match event {
            PpuEvent::EndHDraw => self.ppu_on_end_hdraw(),
            PpuEvent::EndHBlank => self.ppu_on_end_hblank(),
            PpuEvent::EndVBlankHDraw => self.ppu_on_end_vblank_hdraw(),
            PpuEvent::EndVBlankHBlank => self.ppu_on_end_vblank_hblank(),
            PpuEvent::UpdateVCountFlag
            | PpuEvent::RaiseVCountIrq
            | PpuEvent::LatchDispcnt => unreachable!(),
        };
        if lateness > deadline && std::env::var_os("GBA_TRACE_SCHED").is_some() {
            eprintln!(
                "PPU underflow t={} ev={:?} late={} next={:?} delay={} v={}",
                self.scheduler.timestamp(),
                event,
                lateness,
                next_event,
                deadline,
                self.ppu.vcount
            );
        }
        let deadline = deadline.saturating_sub(lateness);
        self.scheduler.push_event(Event::Ppu(next_event), deadline);
    }

    fn ppu_on_end_hdraw(&mut self) -> (PpuEvent, usize) {
        // Draw the line that just finished. All DISPCNT writes made during
        // its HDraw are now known, so it can be split into segments at the
        // exact dot each change took effect. Render before refreshing the
        // OAM snapshot: the snapshot in use is the one fetched for this line.
        if self.should_render {
            self.ppu_render_current_line();
        }

        // The sprite fetch unit has read OAM for the next line during HDraw;
        // HBlank writes are only seen by the line after that.
        self.ppu.oam_render.copy_from_slice(&self.ppu.oam);
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
        self.ppu_begin_line();

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

            // The line is drawn at its EndHDraw, once its DISPCNT writes
            // are all known.
            (PpuEvent::EndHDraw, CYCLES_HDRAW)
        }
    }

    fn ppu_on_end_vblank_hdraw(&mut self) -> (PpuEvent, usize) {
        self.ppu.oam_render.copy_from_slice(&self.ppu.oam);
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
            self.ppu_begin_line();

            (PpuEvent::EndHDraw, CYCLES_HDRAW)
        } else {
            // Another vblank scanline.
            self.update_vcount(new_vcount);
            self.ppu_begin_line();
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

