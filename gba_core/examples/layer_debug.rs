use std::{env, fs};
use gba_core::{Gba, KeypadState, Rom};

fn main() {
    let bios = fs::read("gba_bios.bin").expect("bios");
    let rom = Rom::new(&fs::read("test7.gba").expect("rom"));
    let mut gba = Gba::builder(bios.into(), rom).build();
    for _ in 0..600 { gba.emulate_frame(true); }
    for _ in 0..13 { press(&mut gba, KeypadState::DOWN); }
    press(&mut gba, KeypadState::A);
    run(&mut gba, 10);
    let names = ["Basic Mode 3","Basic Mode 4","Degenerate OBJ transforms","Layer toggle","Layer toggle 2"];
    for (i,name) in names.iter().enumerate() {
        if i != 0 { press(&mut gba, KeypadState::DOWN); }
        press(&mut gba, KeypadState::A);
        run(&mut gba, 8);
        press(&mut gba, KeypadState::START);
        run(&mut gba, 4);
        if i == 4 {
            eprintln!("TRACE FRAME START");
            unsafe { env::set_var("GBA_TRACE_DISPCNT", "1"); }
            gba.emulate_frame(true);
            unsafe { env::remove_var("GBA_TRACE_DISPCNT"); }
            eprintln!("TRACE FRAME END");
            let actual = gba.framebuffer().to_vec();
            press(&mut gba, KeypadState::RIGHT);
            run(&mut gba, 8);
            let expected = gba.framebuffer().to_vec();
            let diff = actual.iter().zip(expected.iter()).filter(|(a,b)| a!=b).count();
            println!("{name}: diff={diff}");
            write_ppm("target/layer_debug_actual.ppm", &actual);
            write_ppm("target/layer_debug_expected.ppm", &expected);
            return;
        }
        let actual = gba.framebuffer().to_vec();
        press(&mut gba, KeypadState::RIGHT);
        run(&mut gba, 8);
        let expected = gba.framebuffer().to_vec();
        let diff = actual.iter().zip(expected.iter()).filter(|(a,b)| a!=b).count();
        println!("{name}: diff={diff}");
        press(&mut gba, KeypadState::START);
        press(&mut gba, KeypadState::B);
        run(&mut gba, 4);
    }
}
fn run(gba: &mut Gba, frames: usize) { for _ in 0..frames { gba.emulate_frame(true); } }
fn write_ppm(path: &str, framebuffer: &[u32]) {
    let mut data = Vec::with_capacity(15 + framebuffer.len() * 3);
    data.extend_from_slice(b"P6\n240 160\n255\n");
    for &pixel in framebuffer {
        data.push((pixel >> 16) as u8); data.push((pixel >> 8) as u8); data.push(pixel as u8);
    }
    fs::write(path, data).unwrap();
}
fn press(gba: &mut Gba, key: u16) {
    gba.set_keypad_state(KeypadState(key)); run(gba,1);
    gba.set_keypad_state(KeypadState::default()); run(gba,2);
}
