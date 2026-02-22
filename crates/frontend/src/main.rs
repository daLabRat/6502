mod app;
mod audio;
mod config;
mod input;
mod menu;
mod screens;
mod system_roms;

fn main() -> eframe::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([400.0, 300.0])
            .with_title("6502 Emulator"),
        ..Default::default()
    };

    eframe::run_native(
        "6502 Emulator",
        options,
        Box::new(|cc| Ok(Box::new(app::EmuApp::new(cc)))),
    )
}
