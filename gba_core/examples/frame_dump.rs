use std::{env, fs};

use gba_core::{Gba, KeypadState, Rom};

fn main() {
    let args: Vec<String> = env::args().collect();
    let rom_path = args
        .get(1)
        .expect("usage: frame_dump <rom> [frames] [out.ppm] [start_frame]");
    let frames: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(300);
    let out_path = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "frame.ppm".to_string());
    let start_frame: Option<usize> = args.get(4).and_then(|s| s.parse().ok());

    let bios = fs::read("gba_bios.bin").expect("bios");
    let rom = Rom::new(&fs::read(rom_path).expect("rom"));
    let mut gba = Gba::builder(bios.into(), rom).build();

    for frame in 0..frames {
        if start_frame == Some(frame + 1) {
            gba.set_keypad_state(KeypadState(KeypadState::START));
        } else if start_frame == Some(frame) {
            gba.set_keypad_state(KeypadState::default());
        }
        gba.emulate_frame(true);
        if matches!(frame + 1, 1 | 30 | 60 | 120 | 300 | 600 | 1200 | 2400) {
            println!("frame {}: {}", frame + 1, gba.debug_status());
        }
    }
    println!("final: {}", gba.debug_status());

    let fb = gba.framebuffer();
    let mut non_black = 0usize;
    let mut colors = std::collections::HashSet::new();
    for &px in fb {
        if px & 0x00FF_FFFF != 0 {
            non_black += 1;
        }
        colors.insert(px);
    }
    println!(
        "frames={} non_black={}/{} unique_colors={}",
        frames,
        non_black,
        fb.len(),
        colors.len()
    );

    let mut ppm = Vec::with_capacity(15 + fb.len() * 3);
    ppm.extend_from_slice(b"P6\n240 160\n255\n");
    for &px in fb {
        ppm.push((px >> 16) as u8);
        ppm.push((px >> 8) as u8);
        ppm.push(px as u8);
    }
    fs::write(&out_path, ppm).expect("write ppm");
    println!("wrote {out_path}");
    for message in gba.debug_messages().iter().take(20) {
        println!("debug: {message:?}");
    }
}
