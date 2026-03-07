mod app;
mod audio;
mod config;
mod input;
mod menu;
mod screens;
mod system_roms;

use std::io::Write;

fn main() -> eframe::Result<()> {
    // WSL2 clipboard crash workaround:
    // winit uses Wayland (WSLg provides WAYLAND_DISPLAY) but arboard opens a separate
    // X11 connection for clipboard. When the X11 server drops that connection the
    // arboard worker thread errors and kills the whole Wayland event loop.
    // Fix: remove DISPLAY so arboard skips X11 and uses Wayland clipboard (or nop),
    // keeping it on the same display server as winit. libxkbcommon-x11 is also not
    // guaranteed to be installed, so forcing X11 winit isn't viable on WSL2.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        // SAFETY: called before any threads are spawned (top of main).
        unsafe { std::env::remove_var("DISPLAY"); }
    }

    // Log to /tmp/emu.log for easy debugging
    let log_file = std::fs::File::create("/tmp/emu.log").expect("Cannot create /tmp/emu.log");
    let log_file = std::sync::Mutex::new(log_file);

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format(move |buf, record| {
            use std::fmt::Write as FmtWrite;
            let mut line = String::new();
            let _ = writeln!(&mut line, "[{} {}] {}",
                record.level(), record.target(), record.args());
            // Write to log file
            if let Ok(mut f) = log_file.lock() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
            write!(buf, "{}", line)
        })
        .init();

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
