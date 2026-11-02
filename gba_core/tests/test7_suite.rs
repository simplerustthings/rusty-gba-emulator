//! Full mGBA GBA Test Suite regression runner.
//!
//! This test is intentionally ignored by default: it needs the proprietary
//! GBA BIOS and the `test7.gba` test ROM from the workspace root. Run it with:
//!
//! ```text
//! cargo test -p gba_core --release --test test7_suite -- --ignored --nocapture
//! ```
//!
//! The baseline below records the current state so new regressions fail even
//! while the remaining accuracy work is still in progress. Once a suite is
//! fixed, update its entry to the new pass count (the eventual target is
//! `passes == total` for every suite).

use std::path::Path;
use std::time::Instant;

use gba_core::{Gba, KeypadState, Rom};

/// (suite index, suite name, expected passes, expected total)
const BASELINE: &[(usize, &str, usize, usize)] = &[
    (0, "Memory tests", 1552, 1552),
    (1, "I/O read tests", 130, 130),
    (2, "Timing tests", 2020, 2020),
    (3, "Timer count-up tests", 936, 936),
    (4, "Timer IRQ tests", 90, 90),
    (5, "Shifter tests", 140, 140),
    (6, "Carry tests", 93, 93),
    (7, "Multiply long tests", 72, 72),
    (8, "BIOS math tests", 615, 615),
    (9, "DMA tests", 1256, 1256),
    (10, "SIO register R/W tests", 90, 90),
    (11, "SIO timing tests", 4, 4),
    (12, "Misc. edge case tests", 5, 12),
    // Suite 13 (Video) is visual-only in test7.gba. It has no END result and
    // is covered by dedicated framebuffer tests instead.
];

#[test]
#[ignore = "requires gba_bios.bin and test7.gba; run with --ignored"]
fn test7_baseline_does_not_regress() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("gba_core should live in the workspace");
    let bios_path = workspace.join("gba_bios.bin");
    let rom_path = workspace.join("test7.gba");
    let (Ok(bios), Ok(rom)) = (std::fs::read(&bios_path), std::fs::read(&rom_path)) else {
        eprintln!(
            "skipping test7 suite: expected {} and {}",
            bios_path.display(),
            rom_path.display()
        );
        return;
    };

    let started = Instant::now();

    // Each suite gets a fresh emulator. The suite's SRAM-backed log is part
    // of the DMA test data, so reusing one session across suites changes the
    // expected DMA results.
    for &(suite_index, suite_name, expected_passes, expected_total) in BASELINE {
        let mut gba = Gba::builder(bios.clone().into(), Rom::new(&rom)).build();

        assert!(
            run_until(&mut gba, 1200, |messages| {
                messages
                    .iter()
                    .any(|message| message.contains("Game Boy Advance Test Suite"))
            }),
            "test7.gba did not boot for suite {suite_index}: {}",
            gba.debug_status()
        );
        run_frames(&mut gba, 10);

        for _ in 0..suite_index {
            press(&mut gba, KeypadState::DOWN);
        }
        press(&mut gba, KeypadState::A);

        assert!(
            run_until(&mut gba, 600, |messages| {
                messages
                    .iter()
                    .any(|message| message.starts_with("BEGIN: "))
            }),
            "suite {suite_index} ({suite_name}) did not start: {}",
            gba.debug_status()
        );

        assert!(
            run_until(&mut gba, 120_000, |messages| {
                messages.iter().any(|message| message.starts_with("END: "))
            }),
            "suite {suite_index} ({suite_name}) did not finish: {}",
            gba.debug_status()
        );

        let end = gba
            .debug_messages()
            .iter()
            .find_map(|message| message.strip_prefix("END: "))
            .expect("END message should exist");
        let (passes, total) = end.split_once('/').expect("END format is passes/total");
        let passes: usize = passes.parse().expect("pass count");
        let total: usize = total.parse().expect("total count");

        println!(
            "{suite_name}: {passes}/{total} (baseline {expected_passes}/{expected_total})"
        );
        assert_eq!(
            (passes, total),
            (expected_passes, expected_total),
            "suite {suite_index} ({suite_name}) regressed or improved; update the baseline"
        );
    }

    println!("test7 baseline run took {:?}", started.elapsed());
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
