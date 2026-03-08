use std::sync::Arc;
use eframe::egui;
use egui::TextureHandle;
use emu_common::SystemEmulator;

use crate::audio::AudioOutput;
use crate::config::Config;
use crate::crt::CrtPipeline;
use crate::debugger::DebuggerState;
use crate::input;
use crate::menu::{self, MenuAction, RecentRoms};
use crate::save_manager::SaveManager;
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
    save_manager: Option<SaveManager>,
    save_name_input: String,
    show_save_dialog: bool,
    show_browse_saves: bool,
    pending_load_slot: Option<u8>,
    pending_load_named: Option<String>,
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
            save_manager: None,
            save_name_input: String::new(),
            show_save_dialog: false,
            show_browse_saves: false,
            pending_load_slot: None,
            pending_load_named: None,
        }
    }

    /// Start a system emulator and switch to the emulation screen.
    fn start_system(&mut self, mut sys: Box<dyn SystemEmulator>, rom_path: Option<&std::path::Path>) {
        if let Some(ref audio) = self.audio {
            sys.set_sample_rate(audio.sample_rate);
        }
        let sys_name = sys.save_state_system_id().to_string();
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

        let saves_root = std::path::PathBuf::from(&self.config.saves_dir);
        self.save_manager = rom_path.map(|p| SaveManager::new(&saves_root, &sys_name, p));
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
                    self.start_system(Box::new(c64), None);
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
                            self.start_system(Box::new(a2), None);
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
            self.load_rom_at_path(system, path);
        }
    }

    fn load_rom_at_path(&mut self, system: SystemChoice, path: std::path::PathBuf) {
        if let Some(parent) = path.parent() {
            self.config.last_rom_dir = Some(parent.to_string_lossy().into_owned());
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
                        let system_id = match system {
                            SystemChoice::Nes       => "NES",
                            SystemChoice::Apple2    => "Apple2",
                            SystemChoice::C64       => "C64",
                            SystemChoice::Atari2600 => "Atari2600",
                        };
                        self.config.push_recent_rom(system_id, &path.to_string_lossy());
                        self.config.save();
                        self.selected_system = Some(system);
                        self.start_system(sys, Some(&path));
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

    fn do_save_slot(&mut self, slot: u8, name: &str) {
        let Some(ref sys) = self.system else { return };
        let Some(ref mut sm) = self.save_manager else {
            self.error_msg = Some("No ROM loaded, cannot save state".into());
            return;
        };
        match sys.save_state() {
            Ok(data) => {
                if let Err(e) = sm.save_to_slot(slot, name, &data) {
                    self.error_msg = Some(format!("Save failed: {}", e));
                }
            }
            Err(e) => self.error_msg = Some(format!("Save state error: {}", e)),
        }
    }

    fn do_load_slot(&mut self, slot: u8) {
        let data = {
            let Some(ref sm) = self.save_manager else { return };
            match sm.load_slot(slot) {
                Ok(d) => d,
                Err(e) => { self.error_msg = Some(format!("Load failed: {}", e)); return; }
            }
        };
        if let Some(ref mut sys) = self.system {
            if let Err(e) = sys.load_state(&data) {
                self.error_msg = Some(format!("Load state error: {}", e));
            }
        }
    }

    fn do_load_named(&mut self, filename: String) {
        let data = {
            let Some(ref sm) = self.save_manager else { return };
            match sm.load_named(&filename) {
                Ok(d) => d,
                Err(e) => { self.error_msg = Some(format!("Load failed: {}", e)); return; }
            }
        };
        if let Some(ref mut sys) = self.system {
            if let Err(e) = sys.load_state(&data) {
                self.error_msg = Some(format!("Load state error: {}", e));
            }
        }
    }
}

impl eframe::App for EmuApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Process pending loads (must happen before input/render)
        if let Some(slot) = self.pending_load_slot.take() {
            self.do_load_slot(slot);
        }
        if let Some(filename) = self.pending_load_named.take() {
            self.do_load_named(filename);
        }

        // Process input
        let input_events = input::process_egui_input(ctx);
        if let Some(ref mut sys) = self.system {
            for event in &input_events {
                sys.handle_input(*event);
            }
        }

        // Build save slot info for menu
        let supports_saves = self.system.as_ref().map_or(false, |s| s.supports_save_states());
        let save_slots: Option<[Option<(String, String)>; 8]> = self.save_manager.as_ref().map(|sm| {
            std::array::from_fn(|i| {
                sm.slot_info(i as u8 + 1).map(|e| (e.name.clone(), e.saved_at.clone()))
            })
        });

        // Top menu
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let recent = RecentRoms {
                nes:    self.config.recent_roms_for("NES"),
                apple2: self.config.recent_roms_for("Apple2"),
                c64:    self.config.recent_roms_for("C64"),
                atari:  self.config.recent_roms_for("Atari2600"),
            };
            let action = menu::render_menu(ui, self.system.is_some(), self.config.crt_mode, save_slots.as_ref(), supports_saves, &recent);
            match action {
                MenuAction::LoadRomForSystem(system) => {
                    self.load_rom(system);
                }
                MenuAction::LoadRecentRom(system, path) => {
                    self.load_rom_at_path(system, std::path::PathBuf::from(&path));
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
                MenuAction::SaveToSlot(slot) => {
                    self.do_save_slot(slot, "");
                }
                MenuAction::LoadFromSlot(slot) => {
                    self.pending_load_slot = Some(slot);
                }
                MenuAction::SaveNamed => {
                    self.show_save_dialog = true;
                }
                MenuAction::BrowseSaves => {
                    self.show_browse_saves = true;
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

        // "Save to new named" dialog
        if self.show_save_dialog {
            egui::Window::new("Save State As")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Save name:");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.save_name_input)
                            .desired_width(200.0)
                            .hint_text("e.g. Before Boss"),
                    );
                    resp.request_focus();
                    ui.horizontal(|ui| {
                        let can_save = !self.save_name_input.trim().is_empty();
                        if ui.add_enabled(can_save, egui::Button::new("Save")).clicked()
                            || (can_save && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        {
                            let name = self.save_name_input.trim().to_string();
                            self.save_name_input.clear();
                            self.show_save_dialog = false;
                            if let (Some(ref sys), Some(ref mut sm)) = (&self.system, &mut self.save_manager) {
                                match sys.save_state() {
                                    Ok(data) => {
                                        if let Err(e) = sm.save_named(&name, &data) {
                                            self.error_msg = Some(format!("Save failed: {}", e));
                                        }
                                    }
                                    Err(e) => self.error_msg = Some(format!("Save error: {}", e)),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_save_dialog = false;
                            self.save_name_input.clear();
                        }
                    });
                });
        }

        // Browse saves window
        if self.show_browse_saves {
            if let Some(ref mut sm) = self.save_manager {
                let named = sm.manifest.named.clone();
                let mut pending_load: Option<String> = None;
                let mut pending_delete: Option<String> = None;
                let mut pending_assign: Option<(String, u8)> = None;

                egui::Window::new("Browse Saves")
                    .collapsible(false)
                    .resizable(true)
                    .default_size([480.0, 300.0])
                    .show(ctx, |ui| {
                        if named.is_empty() {
                            ui.label("No saves yet.");
                        }
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for entry in &named {
                                ui.horizontal(|ui| {
                                    ui.label(&entry.name);
                                    ui.weak(&entry.saved_at);
                                    if let Some(slot) = entry.slot {
                                        ui.label(egui::RichText::new(format!("Slot {}", slot)).weak());
                                    }
                                    if ui.small_button("Load").clicked() {
                                        pending_load = Some(entry.filename.clone());
                                        self.show_browse_saves = false;
                                    }
                                    ui.menu_button("Assign", |ui| {
                                        for s in 1u8..=8 {
                                            if ui.button(format!("Slot {}", s)).clicked() {
                                                pending_assign = Some((entry.filename.clone(), s));
                                                ui.close_menu();
                                            }
                                        }
                                    });
                                    if ui.small_button("Delete").clicked() {
                                        pending_delete = Some(entry.filename.clone());
                                    }
                                });
                            }
                        });
                        ui.separator();
                        if ui.button("Close").clicked() {
                            self.show_browse_saves = false;
                        }
                    });

                if let Some(filename) = pending_load {
                    self.pending_load_named = Some(filename);
                }
                if let Some(filename) = pending_delete {
                    if let Some(ref mut sm) = self.save_manager {
                        let _ = sm.delete_named(&filename);
                    }
                }
                if let Some((filename, slot)) = pending_assign {
                    if let Some(ref mut sm) = self.save_manager {
                        let _ = sm.assign_to_slot(&filename, slot);
                    }
                }
            }
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

        // Debugger window (separate OS window via egui multi-viewport)
        if self.debugger.open {
            if let Some(ref sys) = self.system {
                let snap = crate::debugger::DebugSnapshot::collect(sys.as_ref(), &self.debugger);
                crate::debugger::show(ctx, &mut self.debugger, &snap);
            }
        }

        // Keep repainting at vsync rate during emulation; the accumulator
        // above controls how many step_frame calls happen per repaint.
        if self.system.is_some() {
            ctx.request_repaint();
        }
    }
}
