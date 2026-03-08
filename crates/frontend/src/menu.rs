use egui::Ui;
use crate::crt::CrtMode;

/// Menu action returned by menu rendering.
#[derive(Debug, PartialEq)]
pub enum MenuAction {
    None,
    LoadRom,
    Reset,
    Break,
    Quit,
    BackToSystemSelect,
    SetCrtMode(CrtMode),
    ToggleDebugger,
}

/// Render the menu bar. Returns the action requested, if any.
pub fn render_menu(ui: &mut Ui, has_system: bool, crt_mode: CrtMode) -> MenuAction {
    let mut action = MenuAction::None;

    egui::menu::bar(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Load ROM...").clicked() {
                action = MenuAction::LoadRom;
                ui.close_menu();
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
