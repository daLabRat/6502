use serde::{Deserialize, Serialize};
use crate::crt::CrtMode;

/// Application configuration, persisted to config.ron.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub volume: f32,
    pub last_rom_dir: Option<String>,
    pub window_scale: u32,
    /// Directory containing system ROMs (BIOS/firmware files).
    /// Defaults to `./roms/` next to the executable.
    pub system_roms_dir: String,
    pub crt_mode: CrtMode,
    pub saves_dir: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            volume: 0.7,
            last_rom_dir: None,
            window_scale: 3,
            system_roms_dir: "roms".into(),
            crt_mode: CrtMode::default(),
            saves_dir: "saves".into(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let path = Self::config_path();
        if let Ok(data) = std::fs::read_to_string(&path) {
            ron::from_str(&data).unwrap_or_default()
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = Self::config_path();
        if let Ok(data) = ron::to_string(self) {
            let _ = std::fs::write(path, data);
        }
    }

    fn config_path() -> std::path::PathBuf {
        let mut path = std::env::current_dir().unwrap_or_default();
        path.push("config.ron");
        path
    }
}
