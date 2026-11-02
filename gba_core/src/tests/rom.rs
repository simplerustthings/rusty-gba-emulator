use std::collections::HashSet;

use crate::io::{
    DEBUG_ENABLE_START, DEBUG_FLAG_SEND, DEBUG_FLAGS_START, DEBUG_STRING_START,
};
use crate::tests::harness::load_workspace_file;
use crate::tests::TestGba;

#[test]
fn pinball_boots_to_title_screen() {
    // Pokemon Pinball R/S relies on MSR CPSR_fc changing the T bit as an
    // ARM-to-Thumb trampoline. Before that fix it stayed at a white forced-
    // blank screen and looped in a misdecoded decompressor.
    let (Some(bios), Some(rom)) = (
        load_workspace_file("gba_bios.bin"),
        load_workspace_file("pinball.gba"),
    ) else {
        return;
    };

    let mut test = TestGba::with_rom(&bios, &rom, false);
    test.run_frames(600, true);

    let framebuffer = test.framebuffer();
    let non_black = framebuffer
        .iter()
        .filter(|&&pixel| pixel & 0x00FF_FFFF != 0)
        .count();
    let unique_colors: HashSet<u32> = framebuffer.iter().copied().collect();

    assert!(
        non_black > 5_000,
        "title screen is mostly blank ({non_black}/{} non-black pixels); status: {}",
        framebuffer.len(),
        test.gba.debug_status()
    );
    assert!(
        unique_colors.len() > 32,
        "title screen has too few colors: {}",
        unique_colors.len()
    );
    assert_eq!(
        test.gba.ppu.dispcnt.read() & 0x80,
        0,
        "game remained in forced blank; status: {}",
        test.gba.debug_status()
    );
}

#[test]
fn mgba_debug_console_captures_messages() {
    let mut test = TestGba::new();

    test.write_mem16(DEBUG_ENABLE_START, 0xC0DE);
    assert_eq!(test.read_mem16(DEBUG_ENABLE_START), 0x1DEA);

    for (index, byte) in b"hello from guest".iter().copied().enumerate() {
        test.write_mem8(DEBUG_STRING_START + index as u32, byte);
    }
    test.write_mem16(DEBUG_FLAGS_START, DEBUG_FLAG_SEND);

    assert_eq!(test.debug_messages(), &["hello from guest".to_string()]);
}

#[test]
fn test7_boots_and_opens_the_debug_console() {
    let (Some(bios), Some(rom)) = (
        load_workspace_file("gba_bios.bin"),
        load_workspace_file("test7.gba"),
    ) else {
        return;
    };

    let mut test = TestGba::with_rom(&bios, &rom, false);
    test.run_frames(600, true);

    let messages = test.debug_messages();
    assert!(
        messages
            .iter()
            .any(|message| message.contains("Game Boy Advance Test Suite")),
        "test7 did not reach its main menu; messages: {messages:?}; status: {}",
        test.gba.debug_status()
    );
}
