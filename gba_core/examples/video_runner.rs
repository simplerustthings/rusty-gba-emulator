use std::{env, fs};

use gba_core::{Gba, KeypadState, Rom};

fn main() {
    let bios = fs::read("gba_bios.bin").expect("bios");
    let rom = Rom::new(&fs::read("test7.gba").expect("rom"));
    let mut gba = Gba::builder(bios.into(), rom).build();

    for _ in 0..600 {
        gba.emulate_frame(true);
    }

    // Menu: suite 13 is Video.
    for _ in 0..13 {
        press(&mut gba, KeypadState::DOWN);
    }
    press(&mut gba, KeypadState::A);
    run(&mut gba, 10);

    let names = [
        "Basic Mode 3",
        "Basic Mode 4",
        "Degenerate OBJ transforms",
        "Layer toggle",
        "Layer toggle 2",
        "OAM Update Delay",
        "Window offscreen reset",
    ];

    for (index, name) in names.iter().enumerate() {
        if index != 0 {
            press(&mut gba, KeypadState::DOWN);
        }
        // Enter the selected test.
        press(&mut gba, KeypadState::A);
        run(&mut gba, 8);
        // Hide the navigation sprite, then capture the actual scene.
        press(&mut gba, KeypadState::START);
        run(&mut gba, 4);
        let actual = gba.framebuffer().to_vec();

        // Switch to the expected scene.
        press(&mut gba, KeypadState::RIGHT);
        run(&mut gba, 8);
        let expected = gba.framebuffer().to_vec();

        let diff = actual
            .iter()
            .zip(expected.iter())
            .filter(|(a, b)| a != b)
            .count();
        if diff > 0 {
            write_ppm(&format!("video_{index}_actual.ppm"), &actual);
            write_ppm(&format!("video_{index}_expected.ppm"), &expected);
        }
        let rows: Vec<usize> = (0..160)
            .filter(|&y| {
                actual[y * 240..(y + 1) * 240]
                    .iter()
                    .zip(expected[y * 240..(y + 1) * 240].iter())
                    .any(|(a, b)| a != b)
            })
            .collect();
        println!("{name}: differing pixels = {diff}, rows = {rows:?}");

        // Exit the test and re-enable the navigation sprite.
        press(&mut gba, KeypadState::START);
        press(&mut gba, KeypadState::B);
        run(&mut gba, 4);
    }
}

fn run(gba: &mut Gba, frames: usize) {
    for _ in 0..frames {
        gba.emulate_frame(true);
    }
}

fn write_ppm(path: &str, framebuffer: &[u32]) {
    let mut data = Vec::with_capacity(15 + framebuffer.len() * 3);
    data.extend_from_slice(b"P6\n240 160\n255\n");
    for &pixel in framebuffer {
        data.push((pixel >> 16) as u8);
        data.push((pixel >> 8) as u8);
        data.push(pixel as u8);
    }
    std::fs::write(path, data).unwrap();
}

fn press(gba: &mut Gba, key: u16) {
    gba.set_keypad_state(KeypadState(key));
    run(gba, 1);
    gba.set_keypad_state(KeypadState::default());
    run(gba, 2);
}
