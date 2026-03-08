# ROM Browser (Recent Files) Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create the implementation plan.

**Goal:** Add a persistent "File" menu with per-system recent-file submenus, tracking the last 5 ROMs loaded per system.

**Architecture:** Single-layer — recent paths stored in `Config` (already RON-persisted), surfaced through a new "File" top-level menu. No new files needed beyond config and menu changes.

**Tech Stack:** `egui` submenus, existing `Config`/`ron` persistence, existing `load_rom` path in `app.rs`.

---

## Data

Add to `Config`:

```rust
pub recent_roms: HashMap<String, Vec<String>>,
// keys: "NES", "Apple2", "C64", "Atari2600"
// values: up to 5 absolute paths, most-recent first
```

Default: empty `HashMap`.

Helper on `Config`:

```rust
pub fn push_recent_rom(&mut self, system: &str, path: &str) {
    let list = self.recent_roms.entry(system.to_string()).or_default();
    list.retain(|p| p != path);   // deduplicate
    list.insert(0, path.to_string());
    list.truncate(5);
}
```

Call `push_recent_rom` + `config.save()` immediately after any successful ROM load.

---

## Menu

New "File" top-level menu, always rendered (before "System"):

```
File
  ├── NES ▶
  │     ├── Load ROM…
  │     ├── ─────────────
  │     ├── game1.nes
  │     ├── game2.nes
  │     └── (no recent files)   ← greyed, shown only when list is empty
  ├── Apple II ▶
  │     └── …
  ├── Commodore 64 ▶
  │     └── …
  └── Atari 2600 ▶
        └── …
```

Each entry shows `path.file_name()` (basename only) as the label, with the full path as a tooltip. Clicking a recent entry calls the same load path as the OS picker, passing the stored absolute path directly (no dialog). Clicking "Load ROM…" opens the OS file picker as today.

---

## Update Logic

On successful ROM load (picker or recent):
1. `config.push_recent_rom(system_id, &path.to_string_lossy())`
2. `config.save()`

System IDs match the save-state identifiers: `"NES"`, `"Apple2"`, `"C64"`, `"Atari2600"`.

---

## Error Handling

If a recent path no longer exists when clicked: show the existing `error_msg` window ("Failed to read file: …"). Do not auto-remove stale entries from the list (the file may be on a temporarily unmounted drive).
