//! Dump every debug-console message emitted by one test7.gba suite.
//!
//! Unlike `suite_runner`, this prints PASS lines too, which is useful when
//! grouping failures by test family.
//!
//! ```text
//! cargo run -p gba_core --release --example suite_dump -- test7.gba gba_bios.bin 2
//! ```

use std::{env, fs};

use gba_core::{Gba, KeypadState, Rom};

fn main() {
    let args: Vec<String> = env::args().collect();
    let rom_path = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "test7.gba".to_string());
    let bios_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "gba_bios.bin".to_string());
    let suite_index = args
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    let bios = fs::read(&bios_path).expect("failed to read BIOS");
    let rom = Rom::new(&fs::read(&rom_path).expect("failed to read ROM"));
    let mut gba = Gba::builder(bios.into(), rom).build();

    let booted = run_until(&mut gba, 1200, |messages| {
        messages
            .iter()
            .any(|message| message.contains("Game Boy Advance Test Suite"))
    });
    if !booted {
        eprintln!("test suite did not boot; {}", gba.debug_status());
        std::process::exit(1);
    }
    run_frames(&mut gba, 10);
    for _ in 0..suite_index {
        press(&mut gba, KeypadState::DOWN);
    }
    press(&mut gba, KeypadState::A);

    let begun = run_until(&mut gba, 600, |messages| {
        messages
            .iter()
            .any(|message| message.starts_with("BEGIN: "))
    });
    if !begun {
        eprintln!("suite did not begin; {}", gba.debug_status());
        std::process::exit(1);
    }
    let finished = run_until(&mut gba, 120_000, |messages| {
        messages.iter().any(|message| message.starts_with("END: "))
    });
    if !finished {
        eprintln!("suite did not finish; {}", gba.debug_status());
        std::process::exit(1);
    }

    for message in gba.debug_messages() {
        println!("{message}");
    }
}

fn run_frames(gba: &mut Gba, frames: usize) {
    for _ in 0..frames {
        gba.emulate_frame(true);
    }
}

fn run_until(gba: &mut Gba, max_frames: usize, predicate: impl Fn(&[String]) -> bool) -> bool {
    for _ in 0..max_frames {
        if predicate(gba.debug_messages()) {
            return true;
        }
        gba.emulate_frame(true);
    }
    predicate(gba.debug_messages())
}

fn press(gba: &mut Gba, key: u16) {
    gba.set_keypad_state(KeypadState(key));
    run_frames(gba, 1);
    gba.set_keypad_state(KeypadState::default());
    run_frames(gba, 2);
}
