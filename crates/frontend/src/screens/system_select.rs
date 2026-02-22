/// Which system the user selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemChoice {
    Nes,
    Apple2,
    C64,
    Atari2600,
}

/// What action the user chose on the system select screen.
pub enum SystemAction {
    /// Load a ROM/program file for this system.
    LoadRom(SystemChoice),
    /// Boot with system ROMs only (no game file).
    BootSystem(SystemChoice),
}

/// Render the system selection screen.
pub fn render(ui: &mut egui::Ui) -> Option<SystemAction> {
    let mut action = None;

    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.heading("6502 Multi-System Emulator");
        ui.add_space(30.0);
        ui.label("Select a system:");
        ui.add_space(20.0);

        let button_size = egui::vec2(200.0, 50.0);
        let small_button = egui::vec2(200.0, 30.0);

        // NES - always needs a cartridge
        if ui.add_sized(button_size, egui::Button::new("NES")).clicked() {
            action = Some(SystemAction::LoadRom(SystemChoice::Nes));
        }
        ui.add_space(10.0);

        // Apple II
        if ui.add_sized(button_size, egui::Button::new("Apple II")).clicked() {
            action = Some(SystemAction::LoadRom(SystemChoice::Apple2));
        }
        if ui.add_sized(small_button, egui::Button::new("Boot Apple II (system ROM)")).clicked() {
            action = Some(SystemAction::BootSystem(SystemChoice::Apple2));
        }
        ui.add_space(10.0);

        // C64
        if ui.add_sized(button_size, egui::Button::new("Commodore 64 - Load PRG")).clicked() {
            action = Some(SystemAction::LoadRom(SystemChoice::C64));
        }
        if ui.add_sized(small_button, egui::Button::new("Boot C64 to BASIC")).clicked() {
            action = Some(SystemAction::BootSystem(SystemChoice::C64));
        }
        ui.add_space(10.0);

        // Atari 2600 - always needs a cartridge
        if ui.add_sized(button_size, egui::Button::new("Atari 2600")).clicked() {
            action = Some(SystemAction::LoadRom(SystemChoice::Atari2600));
        }
    });

    action
}
