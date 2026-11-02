//! Temporary helper: run pinball with a button window and save frames.

use std::fs;

use gba_core::{Gba, KeypadState, Rom};

fn save(gba: &Gba, path: &str) {
    let fb = gba.framebuffer();
    let mut ppm = Vec::with_capacity(15 + fb.len() * 3);
    ppm.extend_from_slice(b"P6\n240 160\n255\n");
    for &px in fb {
        ppm.push((px >> 16) as u8);
        ppm.push((px >> 8) as u8);
        ppm.push(px as u8);
    }
    fs::write(path, ppm).expect("write");
}

fn main() {
    let key = std::env::args()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(KeypadState::START);
    let total: usize = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(7200);
    let press_at: usize = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(600);
    let bios = fs::read("gba_bios.bin").expect("bios");
    let rom = Rom::new(&fs::read("pinball.gba").expect("rom"));
    let mut gba = Gba::builder(bios.into(), rom).build();
    let marks = [2400, 3600, 7200, 7500, 8000, 9000];
    for frame in 0..total {
        let press = (frame >= press_at && frame < press_at + 12)
            || (frame >= 7300 && frame < 7312);
        if press {
            gba.set_keypad_state(KeypadState(key));
        } else {
            gba.set_keypad_state(KeypadState::default());
        }
        gba.emulate_frame(true);
        if marks.contains(&(frame + 1)) {
            let path = format!("pin_{key}_{}.ppm", frame + 1);
            save(&gba, &path);
            println!("frame {}: {}\n  wrote {path}", frame + 1, gba.debug_status());
            let vram = gba.debug_vram();
            let pal = gba.debug_palette();
            let vnz = vram.iter().filter(|&&b| b != 0).count();
            let pnz = pal.iter().filter(|&&b| b != 0).count();
            println!(
                "  vram_nonzero={} palette_nonzero={} pal0..16={:02X?} vram0..16={:02X?}",
                vnz,
                pnz,
                &pal[..16],
                &vram[..16]
            );
        }
    }
}
