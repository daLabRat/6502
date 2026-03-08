use std::collections::HashMap;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SlotEntry {
    pub filename: String,
    pub name: String,
    pub saved_at: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct NamedEntry {
    pub filename: String,
    pub name: String,
    pub saved_at: String,
    pub slot: Option<u8>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct SaveManifest {
    pub slots: HashMap<u8, SlotEntry>,
    pub named: Vec<NamedEntry>,
}

pub struct SaveManager {
    dir: PathBuf,
    pub manifest: SaveManifest,
}

impl SaveManager {
    pub fn new(saves_root: &Path, system: &str, rom_path: &Path) -> Self {
        let stem = rom_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        let stem: String = stem.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(64)
            .collect();
        let dir = saves_root.join(system).join(&stem);
        let _ = std::fs::create_dir_all(&dir);
        let manifest = Self::load_manifest(&dir);
        Self { dir, manifest }
    }

    fn manifest_path(dir: &Path) -> PathBuf { dir.join("manifest.ron") }

    fn load_manifest(dir: &Path) -> SaveManifest {
        let path = Self::manifest_path(dir);
        if let Ok(data) = std::fs::read_to_string(&path) {
            ron::from_str(&data).unwrap_or_default()
        } else {
            SaveManifest::default()
        }
    }

    fn save_manifest(&self) {
        let path = Self::manifest_path(&self.dir);
        if let Ok(data) = ron::to_string(&self.manifest) {
            let _ = std::fs::write(path, data);
        }
    }

    fn timestamp() -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let sec = secs % 60; let s = secs / 60;
        let min = s % 60; let s = s / 60;
        let hour = s % 24; let days = s / 24;
        let year = 1970 + days / 365;
        let day_of_year = days % 365;
        let month = (day_of_year / 30 + 1).min(12);
        let day = day_of_year % 30 + 1;
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec)
    }

    pub fn save_to_slot(&mut self, slot: u8, name: &str, data: &[u8]) -> Result<(), String> {
        // Clear any existing named entry that claims this slot
        for ne in &mut self.manifest.named {
            if ne.slot == Some(slot) {
                ne.slot = None;
            }
        }
        let filename = format!("slot{}.state", slot);
        let path = self.dir.join(&filename);
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        let entry = SlotEntry {
            filename: filename.clone(),
            name: if name.is_empty() { format!("Slot {}", slot) } else { name.to_string() },
            saved_at: Self::timestamp(),
        };
        if let Some(ne) = self.manifest.named.iter_mut().find(|n| n.filename == filename) {
            ne.name = entry.name.clone();
            ne.saved_at = entry.saved_at.clone();
            ne.slot = Some(slot);
        } else {
            self.manifest.named.push(NamedEntry {
                filename: filename,
                name: entry.name.clone(),
                saved_at: entry.saved_at.clone(),
                slot: Some(slot),
            });
        }
        self.manifest.slots.insert(slot, entry);
        self.save_manifest();
        Ok(())
    }

    pub fn save_named(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        let safe: String = name.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(48)
            .collect();
        let ts = Self::timestamp();  // capture ONCE
        let ts_compact = ts.replace([':', ' ', '-'], "");
        let filename = format!("named_{}_{}.state", safe, &ts_compact[..8]);
        let path = self.dir.join(&filename);
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        self.manifest.named.push(NamedEntry {
            filename,
            name: name.to_string(),
            saved_at: ts,  // use same timestamp
            slot: None,
        });
        self.save_manifest();
        Ok(())
    }

    pub fn load_slot(&self, slot: u8) -> Result<Vec<u8>, String> {
        let entry = self.manifest.slots.get(&slot)
            .ok_or_else(|| format!("Slot {} is empty", slot))?;
        let path = self.dir.join(&entry.filename);
        std::fs::read(&path).map_err(|e| e.to_string())
    }

    pub fn load_named(&self, filename: &str) -> Result<Vec<u8>, String> {
        let path = self.dir.join(filename);
        // Validate path stays within saves directory
        let dir_canon = self.dir.canonicalize().map_err(|e| e.to_string())?;
        let path_canon = path.canonicalize().map_err(|e| e.to_string())?;
        if !path_canon.starts_with(&dir_canon) {
            return Err("Invalid save filename: path escapes saves directory".into());
        }
        std::fs::read(&path_canon).map_err(|e| e.to_string())
    }

    pub fn assign_to_slot(&mut self, filename: &str, slot: u8) -> Result<(), String> {
        let entry = self.manifest.named.iter_mut()
            .find(|n| n.filename == filename)
            .ok_or_else(|| format!("Save '{}' not found", filename))?;
        entry.slot = Some(slot);
        let slot_entry = SlotEntry {
            filename: entry.filename.clone(),
            name: entry.name.clone(),
            saved_at: entry.saved_at.clone(),
        };
        for ne in &mut self.manifest.named {
            if ne.slot == Some(slot) && ne.filename != filename {
                ne.slot = None;
            }
        }
        // Remove any prior slot mapping for this file
        self.manifest.slots.retain(|_, v| v.filename != filename);
        self.manifest.slots.insert(slot, slot_entry);
        self.save_manifest();
        Ok(())
    }

    pub fn delete_named(&mut self, filename: &str) -> Result<(), String> {
        let path = self.dir.join(filename);
        // Validate path stays within saves directory (only if file exists)
        if path.exists() {
            let dir_canon = self.dir.canonicalize().map_err(|e| e.to_string())?;
            let path_canon = path.canonicalize().map_err(|e| e.to_string())?;
            if !path_canon.starts_with(&dir_canon) {
                return Err("Invalid save filename: path escapes saves directory".into());
            }
            let _ = std::fs::remove_file(&path_canon);
        }
        self.manifest.slots.retain(|_, v| v.filename != filename);
        self.manifest.named.retain(|n| n.filename != filename);
        self.save_manifest();
        Ok(())
    }

    pub fn slot_info(&self, slot: u8) -> Option<&SlotEntry> {
        self.manifest.slots.get(&slot)
    }

    #[allow(dead_code)]
    pub fn all_named(&self) -> &[NamedEntry] {
        &self.manifest.named
    }
}
