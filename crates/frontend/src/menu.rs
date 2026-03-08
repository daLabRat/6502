use egui::Ui;

/// Menu action returned by menu rendering.
#[derive(Debug, PartialEq)]
pub enum MenuAction {
    None,
    LoadRom,
    Reset,
    Break,
    Quit,
    BackToSystemSelect,
    CycleCrtMode,
}

/// Render the menu bar. Returns the action requested, if any.
pub fn render_menu(ui: &mut Ui, has_system: bool, crt_label: &str) -> MenuAction {
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
                if ui.button(format!("Mode: {} →", crt_label)).clicked() {
                    action = MenuAction::CycleCrtMode;
                    ui.close_menu();
                }
            });
        }
    });

    action
}
