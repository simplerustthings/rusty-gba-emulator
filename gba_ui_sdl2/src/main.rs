use std::{fs, thread, time::{Duration, Instant}};

use gba_core::{Gba, KeypadState, AUDIO_CHANNELS, AUDIO_SAMPLE_RATE};

use sdl2::keyboard::{Keycode, Mod, Scancode};
use sdl2::pixels::Color;
use sdl2::controller::{Button, GameController};

const WIDTH: u32 = gba_core::WIDTH as u32;
const HEIGHT: u32 = gba_core::HEIGHT as u32;
const SCALE: u32 = 2;

const GBA_A: usize = 0;
const GBA_B: usize = 1;
const GBA_SELECT: usize = 2;
const GBA_START: usize = 3;
const GBA_RIGHT: usize = 4;
const GBA_LEFT: usize = 5;
const GBA_UP: usize = 6;
const GBA_DOWN: usize = 7;
const GBA_R: usize = 8;
const GBA_L: usize = 9;
const NUM_GBA_BUTTONS: usize = 10;

#[derive(Clone)]
struct InputConfig {
    key_map: [Scancode; NUM_GBA_BUTTONS],
    ctrl_map: [Option<Button>; NUM_GBA_BUTTONS],
    key_ff: Scancode,
    ctrl_ff: Option<Button>,
}

fn default_input_config() -> InputConfig {
    InputConfig {
        key_map: [
            Scancode::Z,
            Scancode::X,
            Scancode::RShift,
            Scancode::Return,
            Scancode::Right,
            Scancode::Left,
            Scancode::Up,
            Scancode::Down,
            Scancode::S,
            Scancode::A,
        ],
        ctrl_map: [
            Some(Button::B),
            Some(Button::A),
            Some(Button::Back),
            Some(Button::Start),
            Some(Button::DPadRight),
            Some(Button::DPadLeft),
            Some(Button::DPadUp),
            Some(Button::DPadDown),
            Some(Button::RightShoulder),
            Some(Button::LeftShoulder),
        ],
        key_ff: Scancode::Tab,
        ctrl_ff: Some(Button::Y),
    }
}

fn parse_ctrl_button(s: &str) -> Option<Button> {
    match s {
        "A" => Some(Button::A),
        "B" => Some(Button::B),
        "X" => Some(Button::X),
        "Y" => Some(Button::Y),
        "Back" => Some(Button::Back),
        "Guide" => Some(Button::Guide),
        "Start" => Some(Button::Start),
        "LeftStick" => Some(Button::LeftStick),
        "RightStick" => Some(Button::RightStick),
        "LeftShoulder" => Some(Button::LeftShoulder),
        "RightShoulder" => Some(Button::RightShoulder),
        "DPadUp" => Some(Button::DPadUp),
        "DPadDown" => Some(Button::DPadDown),
        "DPadLeft" => Some(Button::DPadLeft),
        "DPadRight" => Some(Button::DPadRight),
        _ => None,
    }
}

fn write_default_config(path: &str) {
    let content = "\
# GBA Emulator Input Configuration
# Uncomment and edit lines to change button bindings.
# Keyboard mappings (SDL2 scancode names)
#   key_a       = Z
#   key_b       = X
#   key_select  = RShift
#   key_start   = Return
#   key_right   = Right
#   key_left    = Left
#   key_up      = Up
#   key_down    = Down
#   key_r       = S
#   key_l       = A
#
# Controller mappings (SDL2 controller button names)
#   ctrl_a       = B
#   ctrl_b       = A
#   ctrl_select  = Back
#   ctrl_start   = Start
#   ctrl_right   = DPadRight
#   ctrl_left    = DPadLeft
#   ctrl_up      = DPadUp
#   ctrl_down    = DPadDown
#   ctrl_r       = RightShoulder
#   ctrl_l       = LeftShoulder
#
# Fast Forward key/button (held to speed up emulation)
#   key_ff      = Tab
#   ctrl_ff     = Y
";
    let _ = fs::write(path, content);
}

fn load_input_config(path: &str) -> InputConfig {
    let mut config = default_input_config();
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => {
            write_default_config(path);
            return config;
        }
    };
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, '=');
        let key = parts.next().unwrap().trim();
        let value = parts.next().unwrap_or("").trim();
        if value.is_empty() {
            continue;
        }
        if key == "key_ff" {
            if let Some(sc) = Scancode::from_name(value) {
                config.key_ff = sc;
            }
            continue;
        }
        if key == "ctrl_ff" {
            config.ctrl_ff = parse_ctrl_button(value);
            continue;
        }
        let gba_idx = match key {
            "key_a" | "ctrl_a" => GBA_A,
            "key_b" | "ctrl_b" => GBA_B,
            "key_select" | "ctrl_select" => GBA_SELECT,
            "key_start" | "ctrl_start" => GBA_START,
            "key_right" | "ctrl_right" => GBA_RIGHT,
            "key_left" | "ctrl_left" => GBA_LEFT,
            "key_up" | "ctrl_up" => GBA_UP,
            "key_down" | "ctrl_down" => GBA_DOWN,
            "key_r" | "ctrl_r" => GBA_R,
            "key_l" | "ctrl_l" => GBA_L,
            _ => continue,
        };
        if key.starts_with("key_") {
            if let Some(sc) = Scancode::from_name(value) {
                config.key_map[gba_idx] = sc;
            }
        } else if key.starts_with("ctrl_") {
            config.ctrl_map[gba_idx] = parse_ctrl_button(value);
        }
    }
    println!("Loaded input config from {}", path);
    config
}

fn get_keypad_state(
    event_pump: &sdl2::EventPump,
    controllers: &[GameController],
    config: &InputConfig,
) -> KeypadState {
    let mut state = KeypadState(0);
    for scancode in event_pump.keyboard_state().pressed_scancodes() {
        for (i, &mapped) in config.key_map.iter().enumerate() {
            if scancode == mapped {
                state.0 |= 1 << i;
            }
        }
    }
    for ctrl in controllers {
        for (i, &btn_opt) in config.ctrl_map.iter().enumerate() {
            if let Some(btn) = btn_opt {
                if ctrl.button(btn) {
                    state.0 |= 1 << i;
                }
            }
        }
    }
    state
}

#[derive(Debug)]
struct Args {
    rom_path: String,
    bios_path: String,
    skip_bios: bool,
    bench: Option<usize>,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut rom_path = None;
    let mut bios_path = "gba_bios.bin".to_string();
    let mut skip_bios = false;
    let mut bench = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--bios-path" => {
                i += 1;
                bios_path = args[i].clone();
            }
            "--skip-bios" => {
                skip_bios = true;
            }
            "--bench" => {
                if i + 1 < args.len() && !args[i + 1].starts_with("--") {
                    i += 1;
                    bench = Some(args[i].parse().expect("--bench requires a frame count"));
                } else {
                    bench = Some(20000);
                }
            }
            s if s.starts_with('-') => {
                panic!("Unknown argument: {}", s);
            }
            _ => {
                rom_path = Some(args[i].clone());
            }
        }
        i += 1;
    }

    Args {
        rom_path: rom_path.expect("Usage: gba_ui_sdl2 [--bios-path <path>] [--skip-bios] [--bench <frames>] <rom_path>"),
        bios_path,
        skip_bios,
        bench,
    }
}

fn run_emulator(mut gba: Gba, base_path: &str, config: InputConfig) -> Result<(), String> {
    let save_state_path = format!("{}.save_state", base_path);

    let sdl_context = sdl2::init()?;
    let video_subsystem = sdl_context.video()?;
    let audio_subsystem = sdl_context.audio()?;

    let window = video_subsystem
        .window("GBA", WIDTH * SCALE, HEIGHT * SCALE)
        .opengl()
        .position_centered()
        .allow_highdpi()
        .build()
        .map_err(|e| e.to_string())?;

    let mut canvas = window
        .into_canvas()
        .accelerated()
        .build()
        .map_err(|e| e.to_string())?;
    let texture_creator = canvas.texture_creator();
    let mut texture = texture_creator
        .create_texture_streaming(sdl2::pixels::PixelFormatEnum::ARGB8888, WIDTH, HEIGHT)
        .map_err(|e| e.to_string())?;
    canvas.set_draw_color(Color::RGB(0, 0, 0));
    canvas.clear();

    let display_bounds = video_subsystem.display_bounds(0).map_err(|e| e.to_string())?;
    let display_w = display_bounds.width() as u32;
    let display_h = display_bounds.height() as u32;
    canvas.window_mut().set_fullscreen(sdl2::video::FullscreenType::Desktop).ok();

    let audio_spec_desired = sdl2::audio::AudioSpecDesired {
        freq: Some(gba_core::AUDIO_SAMPLE_RATE as i32),
        channels: Some(2),
        samples: Some(256),
    };
    let audio_device = audio_subsystem.open_queue::<i16, _>(None, &audio_spec_desired)?;
    audio_device.resume();

    let mut frame_counter = 0;
    let mut frame_timer = Instant::now();
    const GBA_FRAME_TIME: Duration = Duration::from_nanos(1_000_000_000 / 60);
    let mut frame_start = Instant::now();
    let mut paused = false;
    let mut single_step = false;
    let mut was_paused = paused;

    let controller_subsystem = sdl_context.game_controller()?;
    let mut controllers: Vec<GameController> = Vec::new();
    if let Ok(num_joysticks) = controller_subsystem.num_joysticks() {
        for index in 0..num_joysticks {
            if controller_subsystem.is_game_controller(index) {
                if let Ok(c) = controller_subsystem.open(index) {
                    controllers.push(c);
                }
            }
        }
    }
    if controllers.len() > 0 {
        println!("Found {} game controller(s)", controllers.len());
    }

    let mut event_pump = sdl_context.event_pump()?;
    let mut last_event: Option<sdl2::event::Event> = None;
    'running: loop {
        loop {
            if last_event.is_none() {
                last_event = event_pump.poll_event();
                if last_event.is_none() {
                    break;
                }
            }
            match last_event.take().unwrap() {
                sdl2::event::Event::Quit { .. } => {
                    break 'running;
                }
                sdl2::event::Event::Window { win_event, .. } => match win_event {
                    sdl2::event::WindowEvent::FocusGained => {
                        paused = was_paused;
                    }
                    sdl2::event::WindowEvent::FocusLost => {
                        was_paused = paused;
                        paused = true;
                    }
                    _ => {}
                },
                sdl2::event::Event::KeyDown {
                    keycode: Some(code),
                    keymod,
                    ..
                } => {
                    let command = keymod == Mod::LGUIMOD || keymod == Mod::RGUIMOD;
                    match code {
                        Keycode::P if command => {
                            paused = !paused;
                        }
                        Keycode::N if command => {
                            paused = true;
                            single_step = true;
                        }
                        Keycode::S if command => {
                            let state = gba.save_state();
                            fs::write(&save_state_path, &state).unwrap();
                            println!("Saved state to {}", &save_state_path);
                        }
                        Keycode::L if command => {
                            match fs::read(&save_state_path) {
                                Ok(data) => {
                                    gba.load_state(&data);
                                    println!("Loaded state from {}", &save_state_path);
                                }
                                Err(_) => println!("Nothing to load."),
                            }
                        }
                        _ => {}
                    }
                }
                sdl2::event::Event::ControllerDeviceAdded { which, .. } => {
                    if controller_subsystem.is_game_controller(which) {
                        if let Ok(c) = controller_subsystem.open(which) {
                            controllers.push(c);
                        }
                    }
                }
                sdl2::event::Event::ControllerDeviceRemoved { which, .. } => {
                    controllers.retain(|c| c.instance_id() != which);
                }
                _ => {}
            }
        }

        let keypad = get_keypad_state(&event_pump, &controllers, &config);
        gba.set_keypad_state(keypad);
        if keypad.0 & 0b1100001111 == 0b1100001111 {
            break 'running;
        }

        let ff_key_down = event_pump.keyboard_state().is_scancode_pressed(config.key_ff);
        let ff_ctrl_down = controllers.iter().any(|c| config.ctrl_ff.map_or(false, |b| c.button(b)));
        let fast_forward = ff_key_down || ff_ctrl_down;
        if !paused || single_step {
            single_step = false;

            let speed = if fast_forward { 4 } else { 1 };
            for i in 0..speed {
                gba.emulate_frame(i == speed - 1);
                frame_counter += 1;
            }

            let src = gba.framebuffer();
            let mut buffer = vec![0u8; src.len() * 4];
            for (out, &pixel) in buffer.chunks_exact_mut(4).zip(src.iter()) {
                out.copy_from_slice(&pixel.to_le_bytes());
            }
            texture
                .update(None, &buffer, (WIDTH * 4) as usize)
                .map_err(|e| e.to_string())?;
            canvas.set_draw_color(Color::RGB(0, 0, 0));
            canvas.clear();
            let scale = (display_w as f64 / WIDTH as f64).min(display_h as f64 / HEIGHT as f64);
            let dst_w = (WIDTH as f64 * scale) as u32;
            let dst_h = (HEIGHT as f64 * scale) as u32;
            let dst_x = (display_w - dst_w) / 2;
            let dst_y = (display_h - dst_h) / 2;
            let dst = sdl2::rect::Rect::new(dst_x as i32, dst_y as i32, dst_w, dst_h);
            canvas.copy(&texture, None, dst)?;
            canvas.present();

            let samples_queued =
                (audio_device.size() as usize) / (std::mem::size_of::<i16>() * AUDIO_CHANNELS);
            let samples_max = 2 * AUDIO_SAMPLE_RATE / 60;
            if samples_queued < samples_max {
                let buffer = gba.audio_buffer();
                let to_add = buffer
                    .len()
                    .min((samples_max - samples_queued) * AUDIO_CHANNELS);
                let _ = audio_device.queue_audio(&buffer[..to_add]);
            }

            let elapsed = frame_start.elapsed();
            if !fast_forward && elapsed < GBA_FRAME_TIME {
                thread::sleep(GBA_FRAME_TIME - elapsed);
            }
            frame_start = Instant::now();
        } else {
            last_event = Some(event_pump.wait_event());
        }

        if Instant::now() - frame_timer > Duration::from_secs(1) {
            canvas
                .window_mut()
                .set_title(&format!("GBA - FPS: {}", frame_counter))
                .map_err(|e| e.to_string())?;
            frame_counter = 0;
            frame_timer = Instant::now();
        }
    }

    Ok(())
}

fn bench_emulator(mut gba: Gba, frames: usize) {
    gba.emulate_frame(true);
    let start = Instant::now();
    for _ in 0..frames {
        gba.emulate_frame(true);
    }
    let elapsed = start.elapsed();
    let fps = frames as f64 / elapsed.as_secs_f64();
    println!(
        "Benchmarked {} frames in {:6.3}s ({:8.1} FPS)",
        frames,
        elapsed.as_secs_f64(),
        fps
    );
}

fn main() {
    let args = parse_args();

    let bios = fs::read(&args.bios_path).expect("failed to read bios");
    assert_eq!(bios.len(), 16 * 1024, "BIOS must be 16 KiB");

    let rom_data = fs::read(&args.rom_path).expect("failed to read ROM");
    let rom = gba_core::Rom::new(&rom_data);
    println!("Loaded {:?}", rom);

    let base_path = if args.rom_path.ends_with(".gba") {
        &args.rom_path[..(args.rom_path.len() - 4)]
    } else {
        &args.rom_path
    };
    let backup_path = format!("{}.sav", base_path);

    if let Some(frames) = args.bench {
        println!("Benchmark: Using cartridge save path {}", backup_path);
        let backup_file = gba_core::util::make_backup_file(backup_path);
        let gba = gba_core::Gba::builder(bios.into(), rom)
            .skip_bios(args.skip_bios)
            .backup_file(backup_file)
            .build();
        bench_emulator(gba, frames);
        return;
    }

    println!("Using cartridge save path {}", backup_path);
    let backup_file = gba_core::util::make_backup_file(backup_path);

    let gba = gba_core::Gba::builder(bios.into(), rom)
        .skip_bios(args.skip_bios)
        .backup_file(backup_file)
        .build();

    let config = load_input_config("gba.cfg");

    run_emulator(gba, base_path, config).unwrap();
}
