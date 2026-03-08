use std::sync::Arc;
use eframe::egui;
use egui::TextureHandle;
use emu_common::SystemEmulator;

use crate::audio::AudioOutput;
use crate::config::Config;
use crate::crt::CrtPipeline;
use crate::debugger::DebuggerState;
use crate::input;
use crate::menu::{self, MenuAction};
use crate::screens::system_select::{SystemAction, SystemChoice};

/// Application screen state.
enum Screen {
    SystemSelect,
    Emulation,
}

/// Main application state.
pub struct EmuApp {
    screen: Screen,
    system: Option<Box<dyn SystemEmulator>>,
    selected_system: Option<SystemChoice>,
    texture: Option<TextureHandle>,
    audio: Option<AudioOutput>,
    config: Config,
    audio_buffer: Vec<f32>,
    error_msg: Option<String>,
    last_frame_time: std::time::Instant,
    frame_accum:     std::time::Duration,
    render_state: Option<Arc<eframe::egui_wgpu::RenderState>>,
    crt: Option<CrtPipeline>,
    debugger: DebuggerState,
}

impl EmuApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let config = Config::load();
        let audio = AudioOutput::new();
        if audio.is_none() {
            log::warn!("Failed to initialize audio output");
        }

        let render_state = _cc.wgpu_render_state.clone().map(Arc::new);

        Self {
            screen: Screen::SystemSelect,
            system: None,
            selected_system: None,
            texture: None,
            audio,
            config,
            audio_buffer: vec![0.0; 2048],
            error_msg: None,
            last_frame_time: std::time::Instant::now(),
            frame_accum:     std::time::Duration::ZERO,
            render_state,
            crt: None,
            debugger: DebuggerState::default(),
        }
    }

    /// Start a system emulator and switch to the emulation screen.
    fn start_system(&mut self, mut sys: Box<dyn SystemEmulator>) {
        if let Some(ref audio) = self.audio {
            sys.set_sample_rate(audio.sample_rate);
        }
        self.system = Some(sys);
        self.screen = Screen::Emulation;
        self.texture = None;
        self.error_msg = None;
        self.last_frame_time = std::time::Instant::now();
        self.frame_accum = std::time::Duration::ZERO;

        if let Some(ref rs) = self.render_state {
            if let Some(ref sys) = self.system {
                let w = sys.display_width();
                let h = sys.display_height();
                self.crt = Some(CrtPipeline::new(rs, w, h));
            }
        }
    }

    /// Boot a system with just system ROMs (no game file).
    /// Currently only meaningful for C64 (boots to BASIC prompt).
    fn boot_system(&mut self, system: SystemChoice) {
        let roms_dir = crate::system_roms::resolve_roms_dir(&self.config.system_roms_dir);

        match system {
            SystemChoice::C64 => {
                let (basic, kernal, chargen, _drive_rom) = crate::system_roms::load_c64_roms(&roms_dir);
                if let (Some(basic), Some(kernal), Some(chargen)) = (basic, kernal, chargen) {
                    let mut c64 = emu_c64::C64::with_roms(&basic, &kernal, &chargen);
                    c64.reset();
                    self.selected_system = Some(system);
                    self.start_system(Box::new(c64));
                } else {
                    self.error_msg = Some(
                        "C64 system ROMs not found. Place basic.rom, kernal.rom, \
                         chargen.rom in roms/c64/"
                            .into(),
                    );
                }
            }
            SystemChoice::Apple2 => {
                if let Some(rom) = crate::system_roms::load_apple2_rom(&roms_dir) {
                    match emu_apple2::Apple2::from_rom(&rom) {
                        Ok(a2) => {
                            self.selected_system = Some(system);
                            self.start_system(Box::new(a2));
                        }
                        Err(e) => self.error_msg = Some(format!("Failed to boot Apple II: {}", e)),
                    }
                } else {
                    self.error_msg = Some(
                        "Apple II ROM not found. Place apple2plus.rom in roms/apple2/".into(),
                    );
                }
            }
            _ => {
                // NES and Atari 2600 always need a cartridge
                self.load_rom(system);
            }
        }
    }

    fn load_rom(&mut self, system: SystemChoice) {
        let filter = match system {
            SystemChoice::Nes => ("NES ROMs", &["nes", "NES"][..]),
            SystemChoice::Apple2 => ("Apple II ROMs", &["rom", "ROM", "bin", "BIN", "dsk", "DSK", "do", "DO", "po", "PO"][..]),
            SystemChoice::C64 => ("C64 Programs", &["prg", "PRG", "rom", "ROM", "bin", "BIN", "t64", "T64", "d64", "D64"][..]),
            SystemChoice::Atari2600 => ("Atari 2600 ROMs", &["a26", "A26", "bin", "BIN", "rom", "ROM"][..]),
        };

        let mut dialog = rfd::FileDialog::new()
            .set_title("Load ROM")
            .add_filter(filter.0, filter.1);

        if let Some(ref dir) = self.config.last_rom_dir {
            dialog = dialog.set_directory(dir);
        }

        if let Some(path) = dialog.pick_file() {
            if let Some(parent) = path.parent() {
                self.config.last_rom_dir = Some(parent.to_string_lossy().into_owned());
                self.config.save();
            }

            match std::fs::read(&path) {
                Ok(data) => {
                    let roms_dir = crate::system_roms::resolve_roms_dir(&self.config.system_roms_dir);

                    let result: Result<Box<dyn SystemEmulator>, String> = match system {
                        SystemChoice::Nes => {
                            emu_nes::Nes::from_rom(&data).map(|n| Box::new(n) as Box<dyn SystemEmulator>)
                        }
                        SystemChoice::Apple2 => {
                            let ext = path.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();

                            if ext == "dsk" || ext == "do" || ext == "po" {
                                // .dsk/.do/.po disk image: need system ROM + disk II ROM
                                let sys_rom = crate::system_roms::load_apple2_rom(&roms_dir);
                                let disk_rom = crate::system_roms::load_disk_ii_rom(&roms_dir);

                                match (sys_rom, disk_rom) {
                                    (Some(sr), Some(dr)) => {
                                        emu_apple2::Apple2::with_disk(&sr, &dr, &data)
                                            .map(|a| Box::new(a) as Box<dyn SystemEmulator>)
                                    }
                                    (None, _) => Err("Apple II system ROM not found. \
                                        Place apple2plus.rom in roms/apple2/".into()),
                                    (_, None) => Err("Disk II ROM not found. \
                                        Place diskII.c600.c6ff.bin in roms/apple2/".into()),
                                }
                            } else {
                                let rom_data = if data.len() >= 8192 {
                                    data.clone()
                                } else if let Some(sys_rom) = crate::system_roms::load_apple2_rom(&roms_dir) {
                                    log::info!("Using system ROM from {}", roms_dir.display());
                                    sys_rom
                                } else {
                                    data.clone()
                                };
                                emu_apple2::Apple2::from_rom(&rom_data)
                                    .map(|a| Box::new(a) as Box<dyn SystemEmulator>)
                            }
                        }
                        SystemChoice::C64 => {
                            let (basic, kernal, chargen, drive_rom) = crate::system_roms::load_c64_roms(&roms_dir);
                            let has_roms = basic.is_some() && kernal.is_some() && chargen.is_some();

                            let ext = path.extension()
                                .and_then(|e| e.to_str())
                                .unwrap_or("")
                                .to_ascii_lowercase();

                            if ext == "d64" {
                                // D64: boot with mounted disk image (needs system ROMs)
                                if has_roms {
                                    emu_c64::C64::from_d64_with_drive_rom(
                                        basic.as_deref().unwrap(),
                                        kernal.as_deref().unwrap(),
                                        chargen.as_deref().unwrap(),
                                        &data,
                                        drive_rom.as_deref(),
                                    ).map(|mut c| {
                                        // Enable IEC trace for the first 6 seconds so
                                        // bus activity is visible in the log file.
                                        c.enable_iec_trace();
                                        Box::new(c) as Box<dyn SystemEmulator>
                                    })
                                } else {
                                    Err("C64 system ROMs required for D64 disk images. \
                                         Place basic.rom, kernal.rom, chargen.rom in roms/c64/".into())
                                }
                            } else {
                                // T64 or PRG: extract PRG data
                                let prg_result = if ext == "t64" {
                                    emu_c64::t64_loader::extract_first_prg(&data)
                                } else {
                                    Ok(data.clone())
                                };

                                match prg_result {
                                    Ok(prg_data) => {
                                        emu_c64::C64::from_rom(&prg_data).map(|mut c| {
                                            if has_roms {
                                                c.load_system_roms(
                                                    basic.as_deref().unwrap(),
                                                    kernal.as_deref().unwrap(),
                                                    chargen.as_deref().unwrap(),
                                                );
                                            } else {
                                                log::warn!(
                                                    "C64 system ROMs not found in {}. \
                                                     Place basic.rom, kernal.rom, chargen.rom in roms/c64/",
                                                    roms_dir.display()
                                                );
                                            }
                                            Box::new(c) as Box<dyn SystemEmulator>
                                        })
                                    }
                                    Err(e) => Err(e),
                                }
                            }
                        }
                        SystemChoice::Atari2600 => {
                            emu_atari2600::Atari2600::from_rom(&data).map(|a| Box::new(a) as Box<dyn SystemEmulator>)
                        }
                    };

                    match result {
                        Ok(sys) => {
                            self.selected_system = Some(system);
                            self.start_system(sys);
                        }
                        Err(e) => {
                            self.error_msg = Some(format!("Failed to load ROM: {}", e));
                        }
                    }
                }
                Err(e) => {
                    self.error_msg = Some(format!("Failed to read file: {}", e));
                }
            }
        }
    }
}

impl eframe::App for EmuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process input
        let input_events = input::process_egui_input(ctx);
        if let Some(ref mut sys) = self.system {
            for event in &input_events {
                sys.handle_input(*event);
            }
        }

        // Top menu
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let action = menu::render_menu(ui, self.system.is_some(), self.config.crt_mode);
            match action {
                MenuAction::LoadRom => {
                    if let Some(system) = self.selected_system {
                        self.load_rom(system);
                    }
                }
                MenuAction::Reset => {
                    if let Some(ref mut sys) = self.system {
                        sys.reset();
                    }
                }
                MenuAction::Break => {
                    if let Some(ref mut sys) = self.system {
                        use emu_common::{Button, InputEvent};
                        sys.handle_input(InputEvent { button: Button::Key(0x03), pressed: true, port: 0 });
                    }
                }
                MenuAction::Quit => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                MenuAction::BackToSystemSelect => {
                    self.screen = Screen::SystemSelect;
                    self.system = None;
                    self.texture = None;
                }
                MenuAction::SetCrtMode(mode) => {
                    self.config.crt_mode = mode;
                    self.config.save();
                }
                MenuAction::ToggleDebugger => {
                    self.debugger.open = !self.debugger.open;
                }
                MenuAction::None => {}
            }
        });

        // Error message
        if let Some(ref msg) = self.error_msg.clone() {
            egui::Window::new("Error").show(ctx, |ui| {
                ui.label(msg);
                if ui.button("OK").clicked() {
                    self.error_msg = None;
                }
            });
        }

        // Main content
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.screen {
                Screen::SystemSelect => {
                    if let Some(action) = crate::screens::system_select::render(ui) {
                        match action {
                            SystemAction::LoadRom(choice) => {
                                self.selected_system = Some(choice);
                                self.load_rom(choice);
                            }
                            SystemAction::BootSystem(choice) => {
                                self.boot_system(choice);
                            }
                        }
                    }
                }
                Screen::Emulation => {
                    if let Some(ref mut sys) = self.system {
                        if self.debugger.paused {
                            // Single-step on request
                            if self.debugger.step {
                                self.debugger.step = false;
                                sys.step_instruction();
                                let pc = sys.cpu_state().pc;
                                self.debugger.check_breakpoint(pc);
                            }
                            // Drain audio so device doesn't underrun
                            let _ = sys.audio_samples(&mut self.audio_buffer);
                        } else {
                            let target = std::time::Duration::from_secs_f64(1.0 / sys.target_fps());
                            let now = std::time::Instant::now();
                            self.frame_accum += now.duration_since(self.last_frame_time);
                            self.last_frame_time = now;
                            // Cap accumulator to avoid spiral-of-death if we fall behind.
                            self.frame_accum = self.frame_accum.min(target * 4);
                            while self.frame_accum >= target {
                                self.frame_accum -= target;
                                sys.step_frame();

                                let count = sys.audio_samples(&mut self.audio_buffer);
                                if let Some(ref mut audio) = self.audio {
                                    audio.push_samples(&self.audio_buffer[..count], self.config.volume);
                                }

                                // Check breakpoints after each frame
                                let pc = sys.cpu_state().pc;
                                self.debugger.check_breakpoint(pc);
                            }
                        }

                        let fb = sys.framebuffer();
                        let aspect = sys.display_aspect_ratio() as f32;
                        let crt_mode = self.config.crt_mode;

                        use crate::crt::CrtMode;
                        if crt_mode != CrtMode::Off {
                            if let Some(ref crt) = self.crt {
                                let available = ui.available_size();
                                let (w, h) = if available.x / available.y > aspect {
                                    (available.y * aspect, available.y)
                                } else {
                                    (available.x, available.x / aspect)
                                };
                                // Record CRT shader into eframe's encoder (no separate submit).
                                let rect = ui.available_rect_before_wrap();
                                ui.painter().add(egui::Shape::Callback(
                                    crt.make_callback(fb.pixels.clone(), crt_mode, rect),
                                ));
                                ui.centered_and_justified(|ui| {
                                    ui.image(egui::load::SizedTexture::new(
                                        crt.texture_id,
                                        egui::vec2(w, h),
                                    ));
                                });
                            } else {
                                crate::screens::emulation::render(ui, &mut self.texture, fb, aspect);
                            }
                        } else {
                            crate::screens::emulation::render(ui, &mut self.texture, fb, aspect);
                        }
                    }
                }
            }
        });

        // Debugger window
        if let Some(ref mut sys) = self.system {
            crate::debugger::render(ctx, &mut self.debugger, sys.as_mut());
        }

        // Keep repainting at vsync rate during emulation; the accumulator
        // above controls how many step_frame calls happen per repaint.
        if self.system.is_some() {
            ctx.request_repaint();
        }
    }
}
