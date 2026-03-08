use std::collections::HashMap;
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
    #[serde(default)]
    pub recent_roms: HashMap<String, Vec<String>>,
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
            recent_roms: HashMap::new(),
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

    /// Add a ROM path to the recent list for a system, deduplicating and capping at 5.
    /// `system_id` should be: "NES", "Apple2", "C64", "Atari2600".
    #[allow(dead_code)]
    pub fn push_recent_rom(&mut self, system_id: &str, path: &str) {
        let list = self.recent_roms.entry(system_id.to_string()).or_default();
        list.retain(|p| p != path);
        list.insert(0, path.to_string());
        list.truncate(5);
    }

    /// Get recent ROMs for a system (most-recent first), or empty slice.
    #[allow(dead_code)]
    pub fn recent_roms_for(&self, system_id: &str) -> &[String] {
        self.recent_roms.get(system_id).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_recent_rom_deduplicates_and_caps_at_5() {
        let mut cfg = Config::default();
        for i in 0..6 {
            cfg.push_recent_rom("NES", &format!("/roms/game{}.nes", i));
        }
        let recent = cfg.recent_roms_for("NES");
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0], "/roms/game5.nes");
        assert!(!recent.contains(&"/roms/game0.nes".to_string()));
    }

    #[test]
    fn push_recent_rom_moves_duplicate_to_front() {
        let mut cfg = Config::default();
        cfg.push_recent_rom("NES", "/roms/a.nes");
        cfg.push_recent_rom("NES", "/roms/b.nes");
        cfg.push_recent_rom("NES", "/roms/a.nes");
        let recent = cfg.recent_roms_for("NES");
        assert_eq!(recent[0], "/roms/a.nes");
        assert_eq!(recent.len(), 2);
    }

    #[test]
    fn recent_roms_for_unknown_system_returns_empty() {
        let cfg = Config::default();
        assert_eq!(cfg.recent_roms_for("Unknown"), &[] as &[String]);
    }
}
