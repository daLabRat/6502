# Save States Design

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:writing-plans to create the implementation plan.

**Goal:** Add save/load state support to all four emulated systems, accessible via menu-driven slot system with named saves.

**Architecture:** Two-layer design — snapshot layer (emulation crates serialize hardware state to `Vec<u8>`) and slot layer (frontend manages files, manifest, and UI). Each hardware component gets a plain-data snapshot struct serialized with `bincode`; function pointers and ROM data are excluded.

**Tech Stack:** `bincode` v2 (new workspace dep), `serde derive` on snapshot structs, `ron` for manifest, `egui` submenus + window for UI.

---

## Snapshot Layer

### Trait additions (`crates/common/src/system.rs`)

```rust
fn save_state(&self) -> Result<Vec<u8>, String> {
    Err("Save states not supported for this system".into())
}
fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    Err("Save states not supported for this system".into())
}
fn supports_save_states(&self) -> bool { false }
```

### File format

16-byte header prepended to every `.state` file:

| Offset | Size | Field           |
|--------|------|-----------------|
| 0      | 4    | Magic `[0x65, 0x6D, 0x75, 0x53]` (`emuS`) |
| 4      | 2    | Version `u16` (start at 1) |
| 6      | 4    | System name CRC32 (reject mismatched loads) |
| 10     | 4    | Snapshot byte length `u32` |
| 14     | 2    | Reserved (zero) |

Snapshot bytes: `bincode` v2 encoded struct.

### CPU snapshot (`crates/cpu/src/`)

```rust
#[derive(Serialize, Deserialize)]
pub struct Cpu6502Snapshot {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,           // flags
    pub total_cycles: u64,
    pub nmi_pending: bool,
    pub irq_pending: bool,
    pub halted: bool,
}
```

`Cpu6502<B>` gets `fn snapshot(&self) -> Cpu6502Snapshot` and `fn restore(&mut self, s: &Cpu6502Snapshot)`. The opcode dispatch table is excluded — rebuilt on `new()`.

### Per-system snapshot structs

**NES** (`crates/nes/src/`):
- `NesCpuSnapshot`: cpu snapshot + `ram: [u8; 2048]`
- `NesPpuSnapshot`: all PPU registers, `vram: [u8; 2048]`, `palette: [u8; 32]`, `oam: [u8; 256]`, scroll/address latches
- `NesApuSnapshot`: all channel register state, frame counter, irq flag
- `NesMapperSnapshot`: `Vec<u8>` — each mapper serializes its own state
- `NesSnapshot`: combines all four

**C64** (`crates/c64/src/`):
- `C64CpuSnapshot`: cpu snapshot + `ram: Box<[u8; 65536]>`
- `VicSnapshot`: all VIC-II registers + color RAM (`[u8; 1024]`), raster line, cycle counter
- `SidSnapshot`: register file `[u8; 29]` + per-voice envelope counters/states
- `CiaSnapshot`: timer A/B counters + latches + control bytes + port values + TOD
- `C64Snapshot`: cpu + vic + sid + cia1 + cia2
- Drive 1541 excluded (too complex; disk image is separate)

**Apple II** (`crates/apple2/src/`):
- `Apple2Snapshot`: cpu snapshot + `ram: Box<[u8; 49152]>` (48KB) + soft switches (bitfield `u16`) + speaker state

**Atari 2600** (`crates/atari2600/src/`):
- `Atari2600Snapshot`: cpu snapshot + `ram: [u8; 128]` + TIA registers `[u8; 64]` + RIOT registers + RIOT RAM `[u8; 128]`

---

## Slot Layer (`crates/frontend/src/`)

### Manifest

```
saves/
  {SystemName}/
    {RomName}/
      manifest.ron
      slot1.state
      named_BeforeBoss.state
      named_Level3.state
```

`{RomName}` is the ROM filename stem, sanitized (alphanumeric + `-_`, max 64 chars).

```rust
// manifest.ron
#[derive(Serialize, Deserialize, Default)]
pub struct SaveManifest {
    pub slots: HashMap<u8, SlotEntry>,   // 1–8
    pub named: Vec<NamedEntry>,
}

#[derive(Serialize, Deserialize)]
pub struct SlotEntry {
    pub filename: String,
    pub name: String,
    pub saved_at: String,   // ISO 8601
}

#[derive(Serialize, Deserialize)]
pub struct NamedEntry {
    pub filename: String,
    pub name: String,
    pub saved_at: String,
    pub slot: Option<u8>,   // if assigned to a slot
}
```

### `SaveManager` struct

```rust
pub struct SaveManager {
    saves_root: PathBuf,
    system: String,
    rom_stem: String,
    manifest: SaveManifest,
}

impl SaveManager {
    pub fn new(saves_root: &Path, system: &str, rom_path: &Path) -> Self;
    pub fn slot_info(&self, slot: u8) -> Option<&SlotEntry>;
    pub fn save_to_slot(&mut self, slot: u8, name: &str, data: &[u8]) -> Result<(), String>;
    pub fn save_named(&mut self, name: &str, data: &[u8]) -> Result<(), String>;
    pub fn load_slot(&self, slot: u8) -> Result<Vec<u8>, String>;
    pub fn load_named(&self, filename: &str) -> Result<Vec<u8>, String>;
    pub fn assign_to_slot(&mut self, filename: &str, slot: u8) -> Result<(), String>;
    pub fn delete_named(&mut self, filename: &str) -> Result<(), String>;
    pub fn all_named(&self) -> &[NamedEntry];
}
```

Manifest is saved to disk after every mutating operation.

`save_to_slot` with an empty name defaults to `"Slot N"`.

### `EmuApp` changes

- Add `save_manager: Option<SaveManager>` field
- Create `SaveManager` in `start_system()` using `config.saves_dir` (new config field, default `"saves"`)
- Add `save_dialog_open: bool` and `browse_saves_open: bool` fields for UI state
- Add `save_name_input: String` for the "save to new named" dialog

---

## UI

### Menu structure

```
System
  ├── Reset
  ├── Break (Ctrl+C)
  ├── ─────────────
  ├── Save State ▶   (greyed if !supports_save_states)
  │     ├── [1] Slot 1: "World 1-1"
  │     ├── [2] Slot 2: (empty)
  │     ├── ...
  │     ├── [8] Slot 8: (empty)
  │     └── ── Save to new named...
  ├── Load State ▶   (greyed if !supports_save_states)
  │     ├── [1] Slot 1: "World 1-1"  2026-03-07 14:32
  │     ├── [2] Slot 2: (empty — disabled)
  │     └── ── Browse all saves...
  └── ─────────────
      Change System
```

### Save to new named dialog

Small `egui::Window` with a text field for the name and Save/Cancel buttons. On Save: calls `save_manager.save_named(name, state_bytes)`.

### Browse saves window

`egui::Window` listing all `named` entries. Columns: Name, Saved At, Slot. Per-row buttons: **Load**, **Assign to slot** (opens a small popup with slot 1–8 picker), **Delete** (with confirmation).

---

## Error handling

- Load failures (wrong system, corrupt data, version mismatch): show existing `error_msg` window.
- Save failures (disk full, permission): same.
- `supports_save_states() = false`: menu items rendered with `ui.add_enabled(false, ...)`.

---

## Dependencies to add

```toml
# workspace Cargo.toml
bincode = { version = "2.0", features = ["serde"] }

# crates/common/Cargo.toml  (already has serde)
# crates/cpu/Cargo.toml
serde = { workspace = true }
bincode = { workspace = true }
# same for nes, apple2, c64, atari2600, frontend
```
