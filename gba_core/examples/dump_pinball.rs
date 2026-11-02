//! Temporary helper: dump EWRAM after running pinball.

use std::fs;

use gba_core::{Gba, Rom};

fn main() {
    let bios = fs::read("gba_bios.bin").expect("bios");
    let rom = Rom::new(&fs::read("pinball.gba").expect("rom"));
    let mut gba = Gba::builder(bios.into(), rom).build();
    for _ in 0..1200 {
        gba.emulate_frame(true);
    }
    println!("{}", gba.debug_status());
    let e = gba.debug_ewram();
    for (i, chunk) in e[0x400..0x500].chunks(16).enumerate() {
        print!("{:05X}:", 0x400 + i * 16);
        for b in chunk {
            print!(" {:02X}", b);
        }
        println!();
    }
}
