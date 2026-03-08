# ROM Browser (Recent Files) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Add a "File" menu with per-system recent-file submenus tracking the last 5 ROMs per system.

**Architecture:** Three changes — (1) add `recent_roms` to `Config`, (2) refactor `load_rom()` into a reusable `load_rom_at_path()` helper + wire recent tracking, (3) replace the File menu's flat "Load ROM" with per-system submenus.

**Tech Stack:** `egui` submenus, `ron`-persisted `Config`, existing `rfd` file dialog.

---

## Task 1: Add `recent_roms` to `Config`

**Files:**
- Modify: `crates/frontend/src/config.rs`

**Context:** `Config` is a `serde`-derived struct serialized to `config.ron` via `ron`. It already has `last_rom_dir: Option<String>`. We add a `HashMap<String, Vec<String>>` for recent ROMs keyed by system ID (`"NES"`, `"Apple2"`, `"C64"`, `"Atari2600"`).

**Step 1: Add `use std::collections::HashMap;` import and the field**

In `crates/frontend/src/config.rs`, add to the imports at the top:
```rust
use std::collections::HashMap;
```

Add the field to `Config`:
```rust
#[serde(default)]
pub recent_roms: HashMap<String, Vec<String>>,
```

The `#[serde(default)]` means existing `config.ron` files without this field will deserialize fine (field defaults to empty HashMap).

Add to `impl Default for Config`:
```rust
recent_roms: HashMap::new(),
```

**Step 2: Add `push_recent_rom` helper**

In `impl Config`, add:
```rust
/// Add a ROM path to the recent list for a system, deduplicating and capping at 5.
/// `system_id` should be the stable save-state identifier: "NES", "Apple2", "C64", "Atari2600".
pub fn push_recent_rom(&mut self, system_id: &str, path: &str) {
    let list = self.recent_roms.entry(system_id.to_string()).or_default();
    list.retain(|p| p != path);
    list.insert(0, path.to_string());
    list.truncate(5);
}

/// Get recent ROMs for a system (most-recent first), or empty slice.
pub fn recent_roms_for(&self, system_id: &str) -> &[String] {
    self.recent_roms.get(system_id).map(|v| v.as_slice()).unwrap_or(&[])
}
```

**Step 3: Write unit tests**

At the bottom of `config.rs`, add:

```rust
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
        assert_eq!(recent[0], "/roms/game5.nes"); // most recent first
        assert!(!recent.contains(&"/roms/game0.nes".to_string())); // oldest dropped
    }

    #[test]
    fn push_recent_rom_moves_duplicate_to_front() {
        let mut cfg = Config::default();
        cfg.push_recent_rom("NES", "/roms/a.nes");
        cfg.push_recent_rom("NES", "/roms/b.nes");
        cfg.push_recent_rom("NES", "/roms/a.nes"); // re-add a
        let recent = cfg.recent_roms_for("NES");
        assert_eq!(recent[0], "/roms/a.nes");
        assert_eq!(recent.len(), 2); // no duplicate
    }

    #[test]
    fn recent_roms_for_unknown_system_returns_empty() {
        let cfg = Config::default();
        assert_eq!(cfg.recent_roms_for("Unknown"), &[] as &[String]);
    }
}
```

**Step 4: Run tests**

```
cargo test -p emu-frontend
```
Expected: 3 new tests pass.

**Step 5: Build**

```
cargo build -p emu-frontend
```
Expected: 0 warnings, 0 errors.

**Step 6: Commit**

```bash
git add crates/frontend/src/config.rs
git commit -m "feat(config): add recent_roms with push/query helpers"
```

---

## Task 2: Refactor `load_rom` + wire recent tracking

**Files:**
- Modify: `crates/frontend/src/app.rs`

**Context:** `load_rom(&mut self, system: SystemChoice)` opens an OS file picker, reads the file, then has ~100 lines of system-specific parsing logic. We extract the post-read work into `load_rom_at_path(&mut self, system: SystemChoice, path: PathBuf)`. Both the picker path and the new recent-file path will call this helper. On success, call `config.push_recent_rom` + `config.save()`.

**Step 1: Extract `load_rom_at_path`**

In `impl EmuApp`, add a new private method. Move the body of `load_rom()` that runs after `dialog.pick_file()` — the part starting from `if let Some(path) = dialog.pick_file() { ... }` — into this new method:

```rust
fn load_rom_at_path(&mut self, system: SystemChoice, path: std::path::PathBuf) {
    // Update last_rom_dir
    if let Some(parent) = path.parent() {
        self.config.last_rom_dir = Some(parent.to_string_lossy().into_owned());
    }

    match std::fs::read(&path) {
        Ok(data) => {
            let roms_dir = crate::system_roms::resolve_roms_dir(&self.config.system_roms_dir);

            let result: Result<Box<dyn SystemEmulator>, String> = match system {
                // ... identical to the existing inner match in load_rom ...
            };

            match result {
                Ok(sys) => {
                    self.selected_system = Some(system);
                    let system_id = match system {
                        SystemChoice::Nes       => "NES",
                        SystemChoice::Apple2    => "Apple2",
                        SystemChoice::C64       => "C64",
                        SystemChoice::Atari2600 => "Atari2600",
                    };
                    self.config.push_recent_rom(system_id, &path.to_string_lossy());
                    self.config.save();
                    self.start_system(sys, Some(&path));
                }
                Err(e) => {
                    self.error_msg = Some(format!("Failed to load ROM: {}", e));
                }
            }
        }
        Err(e) => {
            self.error_msg = Some(format!("Failed to read file: {}", e));
        }
    }
}
```

**Step 2: Simplify `load_rom` to call `load_rom_at_path`**

Replace the body of `load_rom` with:

```rust
fn load_rom(&mut self, system: SystemChoice) {
    let filter = match system {
        SystemChoice::Nes       => ("NES ROMs",       &["nes", "NES"][..]),
        SystemChoice::Apple2    => ("Apple II ROMs",   &["rom", "ROM", "bin", "BIN", "dsk", "DSK", "do", "DO", "po", "PO"][..]),
        SystemChoice::C64       => ("C64 Programs",    &["prg", "PRG", "rom", "ROM", "bin", "BIN", "t64", "T64", "d64", "D64"][..]),
        SystemChoice::Atari2600 => ("Atari 2600 ROMs", &["a26", "A26", "bin", "BIN", "rom", "ROM"][..]),
    };

    let mut dialog = rfd::FileDialog::new()
        .set_title("Load ROM")
        .add_filter(filter.0, filter.1);

    if let Some(ref dir) = self.config.last_rom_dir {
        dialog = dialog.set_directory(dir);
    }

    if let Some(path) = dialog.pick_file() {
        self.load_rom_at_path(system, path);
    }
}
```

**Step 3: Build**

```
cargo build -p emu-frontend
```
Expected: 0 warnings, 0 errors. Behavior is identical to before (refactor only, plus recent tracking added).

**Step 4: Commit**

```bash
git add crates/frontend/src/app.rs
git commit -m "refactor(app): extract load_rom_at_path + wire recent ROM tracking"
```

---

## Task 3: File menu — per-system submenus with recent files

**Files:**
- Modify: `crates/frontend/src/menu.rs`
- Modify: `crates/frontend/src/app.rs`

**Context:** The existing File menu has a flat "Load ROM…" + "Quit". Replace it with four per-system submenus. Each submenu has "Load ROM…" at the top, a separator, then up to 5 recent entries (or a greyed "(no recent files)" if the list is empty). Also need new `MenuAction` variants and to handle them in `app.rs`.

**Step 1: Add new `MenuAction` variants**

In `crates/frontend/src/menu.rs`, add to the `MenuAction` enum:

```rust
LoadRomForSystem(SystemChoice),         // opens OS picker for that system
LoadRecentRom(SystemChoice, String),    // loads from stored path
```

The existing `LoadRom` variant is no longer needed (it was system-agnostic). Remove it.

**Step 2: Update `render_menu` signature**

Add a parameter for recent ROM data. Change the signature to:

```rust
pub fn render_menu(
    ui: &mut Ui,
    has_system: bool,
    crt_mode: CrtMode,
    save_slots: Option<&[Option<(String, String)>; 8]>,
    supports_saves: bool,
    recent: &RecentRoms,
) -> MenuAction
```

Add a helper struct at the top of `menu.rs`:

```rust
use crate::screens::system_select::SystemChoice;

/// Recent ROM lists passed to the menu renderer.
pub struct RecentRoms<'a> {
    pub nes:    &'a [String],
    pub apple2: &'a [String],
    pub c64:    &'a [String],
    pub atari:  &'a [String],
}
```

**Step 3: Replace the File menu content**

Replace the existing File menu block with:

```rust
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
                    let basename = std::path::Path::new(path_str)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path_str.as_str());
                    let resp = ui.button(basename).on_hover_text(path_str);
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
```

**Step 4: Wire in `app.rs`**

In `crates/frontend/src/app.rs`:

a) Build `RecentRoms` before calling `render_menu`:

```rust
let recent = menu::RecentRoms {
    nes:    self.config.recent_roms_for("NES"),
    apple2: self.config.recent_roms_for("Apple2"),
    c64:    self.config.recent_roms_for("C64"),
    atari:  self.config.recent_roms_for("Atari2600"),
};
```

b) Pass `&recent` as the new last argument to `render_menu`.

c) Handle the new action variants in the match (remove the old `MenuAction::LoadRom` arm):

```rust
MenuAction::LoadRomForSystem(system) => {
    self.load_rom(system);
}
MenuAction::LoadRecentRom(system, path) => {
    self.load_rom_at_path(system, std::path::PathBuf::from(&path));
}
```

**Step 5: Build**

```
cargo build --workspace
```
Expected: 0 warnings, 0 errors.

**Step 6: Manual smoke test**

Run the emulator. Verify:
- File menu shows 4 per-system submenus
- Each submenu has "Load ROM…" + "(no recent files)" initially
- After loading a NES ROM via "Load ROM…", it appears in the NES submenu on next launch
- Clicking a recent entry loads it without a file picker
- Up to 5 entries, most-recent first

**Step 7: Commit**

```bash
git add crates/frontend/src/menu.rs crates/frontend/src/app.rs
git commit -m "feat(frontend): per-system recent ROM submenus in File menu"
```

---

## Final verification

```
cargo test --workspace
cargo build --workspace
```

Both must pass with 0 warnings, 0 errors.
