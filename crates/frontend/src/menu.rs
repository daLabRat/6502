use egui::Ui;
use crate::crt::CrtMode;
use crate::screens::system_select::SystemChoice;

/// Recent ROM lists passed to the menu renderer, one slice per system.
pub struct RecentRoms<'a> {
    pub nes:    &'a [String],
    pub apple2: &'a [String],
    pub c64:    &'a [String],
    pub atari:  &'a [String],
}

/// Menu action returned by menu rendering.
#[derive(Debug, PartialEq)]
pub enum MenuAction {
    None,
    LoadRomForSystem(SystemChoice),
    LoadRecentRom(SystemChoice, String),
    Reset,
    Break,
    Quit,
    BackToSystemSelect,
    SetCrtMode(CrtMode),
    ToggleDebugger,
    SaveToSlot(u8),
    LoadFromSlot(u8),
    SaveNamed,
    BrowseSaves,
}

/// Render the menu bar. Returns the action requested, if any.
pub fn render_menu(
    ui: &mut Ui,
    has_system: bool,
    crt_mode: CrtMode,
    save_slots: Option<&[Option<(String, String)>; 8]>,
    supports_saves: bool,
    recent: &RecentRoms<'_>,
) -> MenuAction {
    let mut action = MenuAction::None;

    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            let systems: &[(&str, SystemChoice, &[String])] = &[
                ("NES",          SystemChoice::Nes,       recent.nes),
                ("Apple II",     SystemChoice::Apple2,    recent.apple2),
                ("Commodore 64", SystemChoice::C64,       recent.c64),
                ("Atari 2600",   SystemChoice::Atari2600, recent.atari),
            ];
            for (label, system, recents) in systems {
                ui.menu_button(*label, |ui| {
                    if ui.button("Load ROM...").clicked() {
                        action = MenuAction::LoadRomForSystem(*system);
                        ui.close_menu();
                    }
                    ui.separator();
                    if recents.is_empty() {
                        ui.add_enabled(false, egui::Button::new("(no recent files)"));
                    } else {
                        for path_str in recents.iter() {
                            let basename = std::path::Path::new(path_str.as_str())
                                .file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or(path_str.as_str());
                            let resp = ui.button(basename).on_hover_text(path_str.as_str());
                            if resp.clicked() {
                                action = MenuAction::LoadRecentRom(*system, path_str.clone());
                                ui.close_menu();
                            }
                        }
                    }
                });
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                action = MenuAction::Quit;
                ui.close_menu();
            }
        });

        if has_system {
            if ui.button("Debugger").clicked() {
                action = MenuAction::ToggleDebugger;
            }

            ui.menu_button("System", |ui| {
                if ui.button("Reset").clicked() {
                    action = MenuAction::Reset;
                    ui.close_menu();
                }
                if ui.button("Break (Ctrl+C)").clicked() {
                    action = MenuAction::Break;
                    ui.close_menu();
                }
                ui.separator();
                if ui.button("Change System").clicked() {
                    action = MenuAction::BackToSystemSelect;
                    ui.close_menu();
                }
                if supports_saves {
                    ui.separator();
                    ui.menu_button("Save State", |ui| {
                        for slot in 1u8..=8 {
                            let label = if let Some(Some((name, _date))) = save_slots.map(|s| &s[(slot-1) as usize]) {
                                format!("[{}] {}", slot, name)
                            } else {
                                format!("[{}] (empty)", slot)
                            };
                            if ui.button(&label).clicked() {
                                action = MenuAction::SaveToSlot(slot);
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui.button("Save to new named...").clicked() {
                            action = MenuAction::SaveNamed;
                            ui.close_menu();
                        }
                    });
                    ui.menu_button("Load State", |ui| {
                        for slot in 1u8..=8 {
                            let info = save_slots.and_then(|s| s[(slot-1) as usize].as_ref());
                            if let Some((name, date)) = info {
                                if ui.button(format!("[{}] {}  {}", slot, name, date)).clicked() {
                                    action = MenuAction::LoadFromSlot(slot);
                                    ui.close_menu();
                                }
                            } else {
                                ui.add_enabled(false, egui::Button::new(format!("[{}] (empty)", slot)));
                            }
                        }
                        ui.separator();
                        if ui.button("Browse all saves...").clicked() {
                            action = MenuAction::BrowseSaves;
                            ui.close_menu();
                        }
                    });
                } else {
                    ui.add_enabled(false, egui::Button::new("Save State"));
                    ui.add_enabled(false, egui::Button::new("Load State"));
                }
            });

            ui.menu_button("Display", |ui| {
                for mode in [CrtMode::Off, CrtMode::Sharp, CrtMode::Scanlines, CrtMode::Crt] {
                    if ui.selectable_label(crt_mode == mode, mode.label()).clicked() {
                        action = MenuAction::SetCrtMode(mode);
                        ui.close_menu();
                    }
                }
            });
        }
    });

    action
}
