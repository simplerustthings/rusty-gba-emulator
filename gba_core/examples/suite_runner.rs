use std::{env, fs, path::Path};

use gba_core::{Gba, KeypadState, Rom};

const SUITES: [&str; 14] = [
    "Memory tests",
    "I/O read tests",
    "Timing tests",
    "Timer count-up tests",
    "Timer IRQ tests",
    "Shifter tests",
    "Carry tests",
    "Multiply long tests",
    "BIOS math tests",
    "DMA tests",
    "SIO register R/W tests",
    "SIO timing tests",
    "Misc. edge case tests",
    "Video tests",
];

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

    let bios = fs::read(&bios_path).expect("failed to read BIOS");
    let rom = Rom::new(&fs::read(&rom_path).expect("failed to read ROM"));
    let mut gba = Gba::builder(bios.into(), rom).build();

    // Boot through the BIOS. The suite writes its banner once the cartridge
    // code starts running.
    let booted = run_until(&mut gba, 1200, |messages| {
        messages
            .iter()
            .any(|message| message.contains("Game Boy Advance Test Suite"))
    });
    if !booted {
        eprintln!("test suite did not boot; {}", gba.debug_status());
        std::process::exit(1);
    }

    // Let the menu draw and settle.
    run_frames(&mut gba, 10);

    let suite_index = args
        .get(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    if suite_index >= SUITES.len() {
        panic!("suite index out of range");
    }

    println!("running suite {suite_index}: {}", SUITES[suite_index]);
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

    let messages = gba.debug_messages();
    for message in messages {
        if message.starts_with("BEGIN: ")
            || message.starts_with("END: ")
            || message.contains("FAIL")
            || message.contains("TIMED OUT")
            || message.starts_with("SKIPPED")
        {
            println!("{message}");
        }
    }

    let passes = messages
        .iter()
        .filter(|message| message.starts_with("PASS: "))
        .count();
    let failures = messages
        .iter()
        .filter(|message| message.starts_with("FAIL: ") || message.contains(": FAIL"))
        .count();
    println!("passes={passes} failures={failures}");
    if failures > 0 {
        std::process::exit(2);
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

// Keep this referenced so the runner's intended ROM path is obvious.
#[allow(dead_code)]
fn default_rom_path() -> &'static Path {
    Path::new("test7.gba")
}
