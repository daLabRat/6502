mod app;
mod audio;
mod config;
mod crt;
mod debugger;
mod input;
mod menu;
mod screens;
mod system_roms;

use std::io::Write;

fn main() -> eframe::Result<()> {
    // WSL2 display backend fix:
    // Force X11 on WSL2 to avoid two issues:
    // 1. wgpu's Vulkan Wayland surface integration is unstable in WSLg, causing
    //    broken pipe errors and ExitFailure(1) from the Wayland event loop.
    // 2. arboard (clipboard) uses X11 while winit would use Wayland — when the
    //    X11 connection drops, arboard's worker thread kills the Wayland event loop.
    // X11 avoids both: winit, wgpu, and arboard all use the same XWayland backend.
    // Requires: sudo apt install libxkbcommon-x11-0
    #[cfg(target_os = "linux")]
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        // SAFETY: called before any threads are spawned (top of main).
        unsafe { std::env::remove_var("WAYLAND_DISPLAY"); }
    }

    // Log to /tmp/emu.log for easy debugging
    let log_file = std::fs::File::create("/tmp/emu.log").expect("Cannot create /tmp/emu.log");
    let log_file = std::sync::Mutex::new(log_file);

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info,emu_c64::drive1541=debug"))
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
