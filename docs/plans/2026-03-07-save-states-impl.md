# Save States Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task.

**Goal:** Add save/load state to all four emulated systems with menu-driven slot management and named saves.

**Architecture:** Two-layer design — each system exposes `save_state() -> Vec<u8>` / `load_state(&[u8])` that serialize plain-data snapshot structs via `bincode`; a frontend `SaveManager` handles files, slots, and a `manifest.ron` per ROM.

**Tech Stack:** `bincode` v2 (new), `serde` derive (existing), `ron` (existing), `egui` submenus + window.

**Design doc:** `docs/plans/2026-03-07-save-states-design.md`

---

## Task 1: Add bincode to workspace

**Files:**
- Modify: `Cargo.toml`
- Modify: `crates/cpu/Cargo.toml`
- Modify: `crates/nes/Cargo.toml`
- Modify: `crates/apple2/Cargo.toml`
- Modify: `crates/c64/Cargo.toml`
- Modify: `crates/atari2600/Cargo.toml`
- Modify: `crates/frontend/Cargo.toml`

**Step 1: Add bincode to workspace Cargo.toml**

```toml
# In [workspace.dependencies]:
bincode = { version = "2", features = ["serde"] }
```

**Step 2: Add serde + bincode to each emulation crate**

For `crates/cpu/Cargo.toml`, `crates/nes/Cargo.toml`, `crates/apple2/Cargo.toml`, `crates/c64/Cargo.toml`, `crates/atari2600/Cargo.toml`:
```toml
[dependencies]
serde = { workspace = true }
bincode = { workspace = true }
```

For `crates/frontend/Cargo.toml` (already has serde):
```toml
[dependencies]
bincode = { workspace = true }
```

**Step 3: Verify build passes**

```
cargo build --workspace
```
Expected: 0 errors, 0 warnings.

**Step 4: Commit**

```bash
git add Cargo.toml Cargo.lock crates/cpu/Cargo.toml crates/nes/Cargo.toml crates/apple2/Cargo.toml crates/c64/Cargo.toml crates/atari2600/Cargo.toml crates/frontend/Cargo.toml
git commit -m "feat: add bincode v2 workspace dependency for save states"
```

---

## Task 2: CPU snapshot struct

**Files:**
- Create: `crates/cpu/src/snapshot.rs`
- Modify: `crates/cpu/src/lib.rs`
- Modify: `crates/cpu/src/cpu.rs`

**Context:** `Cpu6502<B: Bus>` has fields: `pc: u16`, `sp: u8`, `a: u8`, `x: u8`, `y: u8`, `p: StatusFlags` (bitflags), `bcd_enabled: bool`, `cmos_mode: bool`, `total_cycles: u64`, `jammed: bool`. The opcode dispatch table is NOT serialized — it's rebuilt on `new()`.

**Step 1: Create `crates/cpu/src/snapshot.rs`**

```rust
use serde::{Serialize, Deserialize};

/// Plain-data snapshot of Cpu6502 register state.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Cpu6502Snapshot {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,           // StatusFlags bits
    pub bcd_enabled: bool,
    pub cmos_mode: bool,
    pub total_cycles: u64,
    pub jammed: bool,
}
```

**Step 2: Add snapshot/restore to `Cpu6502` in `crates/cpu/src/cpu.rs`**

Add these methods to the `impl<B: Bus> Cpu6502<B>` block:

```rust
pub fn snapshot(&self) -> crate::snapshot::Cpu6502Snapshot {
    crate::snapshot::Cpu6502Snapshot {
        pc: self.pc,
        sp: self.sp,
        a: self.a,
        x: self.x,
        y: self.y,
        p: self.p.bits(),
        bcd_enabled: self.bcd_enabled,
        cmos_mode: self.cmos_mode,
        total_cycles: self.total_cycles,
        jammed: self.jammed,
    }
}

pub fn restore(&mut self, s: &crate::snapshot::Cpu6502Snapshot) {
    self.pc = s.pc;
    self.sp = s.sp;
    self.a = s.a;
    self.x = s.x;
    self.y = s.y;
    self.p = crate::flags::StatusFlags::from_bits_truncate(s.p);
    self.bcd_enabled = s.bcd_enabled;
    self.cmos_mode = s.cmos_mode;
    self.total_cycles = s.total_cycles;
    self.jammed = s.jammed;
}
```

**Step 3: Export from `crates/cpu/src/lib.rs`**

```rust
pub mod snapshot;
pub use snapshot::Cpu6502Snapshot;
```

**Step 4: Write unit test in `crates/cpu/src/snapshot.rs`**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_cpu_snapshot() {
        let snap = Cpu6502Snapshot {
            pc: 0x1234, sp: 0xFD, a: 0xAB, x: 0x01, y: 0x02,
            p: 0x24, bcd_enabled: true, cmos_mode: false,
            total_cycles: 12345, jammed: false,
        };
        let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard()).unwrap();
        let (decoded, _): (Cpu6502Snapshot, _) =
            bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).unwrap();
        assert_eq!(decoded.pc, 0x1234);
        assert_eq!(decoded.total_cycles, 12345);
    }
}
```

**Step 5: Run test**

```
cargo test -p emu-cpu snapshot
```
Expected: 1 test passes.

**Step 6: Commit**

```bash
git add crates/cpu/src/snapshot.rs crates/cpu/src/lib.rs crates/cpu/src/cpu.rs
git commit -m "feat(cpu): add Cpu6502Snapshot + snapshot/restore methods"
```

---

## Task 3: SystemEmulator trait additions + save format helper

**Files:**
- Modify: `crates/common/src/system.rs`
- Create: `crates/common/src/save_format.rs`
- Modify: `crates/common/src/lib.rs`

**Context:** Add three methods to `SystemEmulator` with default impls that return errors. Also add a header encode/decode helper used by all systems.

**Step 1: Create `crates/common/src/save_format.rs`**

```rust
/// Magic bytes identifying a save state file.
pub const MAGIC: [u8; 4] = [0x65, 0x6D, 0x75, 0x53]; // "emuS"
/// Current save state format version.
pub const VERSION: u16 = 1;

/// Wrap snapshot bytes with a 16-byte header.
///
/// Header layout (16 bytes):
/// [0..4]  Magic "emuS"
/// [4..6]  Version u16 LE
/// [6..10] System CRC32 (simple hash of system name)
/// [10..14] Payload length u32 LE
/// [14..16] Reserved (zero)
pub fn encode(system_name: &str, snapshot_bytes: &[u8]) -> Vec<u8> {
    let crc = name_crc32(system_name);
    let mut out = Vec::with_capacity(16 + snapshot_bytes.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(snapshot_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&[0u8; 2]); // reserved
    out.extend_from_slice(snapshot_bytes);
    out
}

/// Strip the header and return the snapshot bytes, or an error.
pub fn decode<'a>(system_name: &str, data: &'a [u8]) -> Result<&'a [u8], String> {
    if data.len() < 16 {
        return Err("Save state too small".into());
    }
    if data[0..4] != MAGIC {
        return Err("Invalid save state (bad magic)".into());
    }
    let version = u16::from_le_bytes([data[4], data[5]]);
    if version != VERSION {
        return Err(format!("Save state version mismatch (got {}, expected {})", version, VERSION));
    }
    let file_crc = u32::from_le_bytes([data[6], data[7], data[8], data[9]]);
    let expected_crc = name_crc32(system_name);
    if file_crc != expected_crc {
        return Err("Save state is for a different system".into());
    }
    let len = u32::from_le_bytes([data[10], data[11], data[12], data[13]]) as usize;
    if data.len() < 16 + len {
        return Err("Save state truncated".into());
    }
    Ok(&data[16..16 + len])
}

/// Simple polynomial hash of a string, used as system identifier in save headers.
fn name_crc32(name: &str) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in name.bytes() {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrip() {
        let payload = b"hello world snapshot";
        let wrapped = encode("NES", payload);
        assert_eq!(wrapped.len(), 16 + payload.len());
        let decoded = decode("NES", &wrapped).unwrap();
        assert_eq!(decoded, payload);
    }

    #[test]
    fn wrong_system_rejected() {
        let payload = b"nes data";
        let wrapped = encode("NES", payload);
        assert!(decode("C64", &wrapped).is_err());
    }
}
```

**Step 2: Export from `crates/common/src/lib.rs`**

```rust
pub mod save_format;
pub use save_format::{encode as save_encode, decode as save_decode};
```

**Step 3: Add trait methods to `crates/common/src/system.rs`**

```rust
/// Serialize the complete emulator state to a byte blob.
/// Returns Err if save states are not supported for this system.
fn save_state(&self) -> Result<Vec<u8>, String> {
    Err("Save states not supported for this system".into())
}

/// Restore emulator state from a byte blob previously returned by `save_state`.
/// Returns Err on version mismatch, data corruption, or unsupported system.
fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let _ = data;
    Err("Save states not supported for this system".into())
}

/// Returns true if this system implements save/load state.
fn supports_save_states(&self) -> bool { false }
```

**Step 4: Run tests**

```
cargo test -p emu-common
```
Expected: 2 new tests pass.

**Step 5: Build workspace**

```
cargo build --workspace
```
Expected: 0 warnings, 0 errors.

**Step 6: Commit**

```bash
git add crates/common/src/save_format.rs crates/common/src/system.rs crates/common/src/lib.rs
git commit -m "feat(common): add save_state/load_state trait methods + header format"
```

---

## Task 4: Atari 2600 save states

**Files:**
- Create: `crates/atari2600/src/snapshot.rs`
- Modify: `crates/atari2600/src/lib.rs`

**Context:** Atari 2600 has `Cpu6502<Atari2600Bus>`. `Atari2600Bus` owns `Tia` and `Riot`. Snapshot everything except `framebuffer`, `sample_buffer`, `sample_rate`, `sample_accum` (audio bookkeeping, reset on load is fine). System name for CRC: `"Atari2600"`.

**Step 1: Create `crates/atari2600/src/snapshot.rs`**

```rust
use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct AudioChannelSnapshot {
    pub audc: u8,
    pub audf: u8,
    pub audv: u8,
    pub freq_counter: u8,
    pub poly4: u8,
    pub poly5: u8,
    pub poly9: u16,
    pub div_counter: u8,
    pub output: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TiaSnapshot {
    pub pf0: u8, pub pf1: u8, pub pf2: u8,
    pub pf_reflect: bool, pub pf_score: bool, pub pf_priority: bool,
    pub grp0: u8, pub grp1: u8, pub grp0_old: u8, pub grp1_old: u8,
    pub resp0: u8, pub resp1: u8,
    pub refp0: bool, pub refp1: bool,
    pub vdelp0: bool, pub vdelp1: bool,
    pub enam0: bool, pub enam1: bool,
    pub resm0: u8, pub resm1: u8,
    pub resmp0: bool, pub resmp1: bool,
    pub enabl: bool, pub enabl_old: bool, pub resbl: u8, pub vdelbl: bool,
    pub colup0: u8, pub colup1: u8, pub colupf: u8, pub colubk: u8,
    pub nusiz0: u8, pub nusiz1: u8, pub ctrlpf: u8,
    pub hmp0: i8, pub hmp1: i8, pub hmm0: i8, pub hmm1: i8, pub hmbl: i8,
    pub hmove_pending: bool, pub hmove_blanking: u8,
    pub resp0_delay: u8, pub resp0_pending: u8,
    pub resp1_delay: u8, pub resp1_pending: u8,
    pub resm0_delay: u8, pub resm0_pending: u8,
    pub resm1_delay: u8, pub resm1_pending: u8,
    pub resbl_delay: u8, pub resbl_pending: u8,
    pub inpt4: bool, pub inpt5: bool,
    pub collision: [u8; 8],
    pub scanline: u16, pub clock: u16,
    pub wsync: bool, pub vsync: bool, pub vblank: bool, pub frame_ready: bool,
    pub audio_ch: [AudioChannelSnapshot; 2],
}

#[derive(Serialize, Deserialize)]
pub struct RiotSnapshot {
    pub ram: [u8; 128],
    pub swcha: u8, pub swcha_out: u8, pub swacnt: u8,
    pub swchb: u8, pub swbcnt: u8,
    pub timer_value: u8,
    pub timer_interval: u32,
    pub timer_subcycles: u32,
    pub timer_expired: bool,
    pub timer_flag: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Atari2600Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub tia: TiaSnapshot,
    pub riot: RiotSnapshot,
}
```

**Step 2: Add snapshot/restore to TIA**

In `crates/atari2600/src/tia/mod.rs`, add a `pub(crate) mod` import and add methods to `impl Tia`:

```rust
use crate::snapshot::{AudioChannelSnapshot, TiaSnapshot};

impl Tia {
    pub fn snapshot(&self) -> TiaSnapshot {
        let ch = |c: &AudioChannel| AudioChannelSnapshot {
            audc: c.audc, audf: c.audf, audv: c.audv,
            freq_counter: c.freq_counter,
            poly4: c.poly4, poly5: c.poly5, poly9: c.poly9,
            div_counter: c.div_counter, output: c.output,
        };
        TiaSnapshot {
            pf0: self.pf0, pf1: self.pf1, pf2: self.pf2,
            pf_reflect: self.pf_reflect, pf_score: self.pf_score, pf_priority: self.pf_priority,
            grp0: self.grp0, grp1: self.grp1, grp0_old: self.grp0_old, grp1_old: self.grp1_old,
            resp0: self.resp0, resp1: self.resp1,
            refp0: self.refp0, refp1: self.refp1,
            vdelp0: self.vdelp0, vdelp1: self.vdelp1,
            enam0: self.enam0, enam1: self.enam1,
            resm0: self.resm0, resm1: self.resm1,
            resmp0: self.resmp0, resmp1: self.resmp1,
            enabl: self.enabl, enabl_old: self.enabl_old, resbl: self.resbl, vdelbl: self.vdelbl,
            colup0: self.colup0, colup1: self.colup1, colupf: self.colupf, colubk: self.colubk,
            nusiz0: self.nusiz0, nusiz1: self.nusiz1, ctrlpf: self.ctrlpf,
            hmp0: self.hmp0, hmp1: self.hmp1, hmm0: self.hmm0, hmm1: self.hmm1, hmbl: self.hmbl,
            hmove_pending: self.hmove_pending, hmove_blanking: self.hmove_blanking,
            resp0_delay: self.resp0_delay, resp0_pending: self.resp0_pending,
            resp1_delay: self.resp1_delay, resp1_pending: self.resp1_pending,
            resm0_delay: self.resm0_delay, resm0_pending: self.resm0_pending,
            resm1_delay: self.resm1_delay, resm1_pending: self.resm1_pending,
            resbl_delay: self.resbl_delay, resbl_pending: self.resbl_pending,
            inpt4: self.inpt4, inpt5: self.inpt5,
            collision: self.collision,
            scanline: self.scanline, clock: self.clock,
            wsync: self.wsync, vsync: self.vsync, vblank: self.vblank, frame_ready: self.frame_ready,
            audio_ch: [ch(&self.audio_ch[0]), ch(&self.audio_ch[1])],
        }
    }

    pub fn restore(&mut self, s: &TiaSnapshot) {
        let rc = |c: &mut AudioChannel, sc: &AudioChannelSnapshot| {
            c.audc = sc.audc; c.audf = sc.audf; c.audv = sc.audv;
            c.freq_counter = sc.freq_counter;
            c.poly4 = sc.poly4; c.poly5 = sc.poly5; c.poly9 = sc.poly9;
            c.div_counter = sc.div_counter; c.output = sc.output;
        };
        self.pf0 = s.pf0; self.pf1 = s.pf1; self.pf2 = s.pf2;
        self.pf_reflect = s.pf_reflect; self.pf_score = s.pf_score; self.pf_priority = s.pf_priority;
        self.grp0 = s.grp0; self.grp1 = s.grp1; self.grp0_old = s.grp0_old; self.grp1_old = s.grp1_old;
        self.resp0 = s.resp0; self.resp1 = s.resp1;
        self.refp0 = s.refp0; self.refp1 = s.refp1;
        self.vdelp0 = s.vdelp0; self.vdelp1 = s.vdelp1;
        self.enam0 = s.enam0; self.enam1 = s.enam1;
        self.resm0 = s.resm0; self.resm1 = s.resm1;
        self.resmp0 = s.resmp0; self.resmp1 = s.resmp1;
        self.enabl = s.enabl; self.enabl_old = s.enabl_old; self.resbl = s.resbl; self.vdelbl = s.vdelbl;
        self.colup0 = s.colup0; self.colup1 = s.colup1; self.colupf = s.colupf; self.colubk = s.colubk;
        self.nusiz0 = s.nusiz0; self.nusiz1 = s.nusiz1; self.ctrlpf = s.ctrlpf;
        self.hmp0 = s.hmp0; self.hmp1 = s.hmp1; self.hmm0 = s.hmm0; self.hmm1 = s.hmm1; self.hmbl = s.hmbl;
        self.hmove_pending = s.hmove_pending; self.hmove_blanking = s.hmove_blanking;
        self.resp0_delay = s.resp0_delay; self.resp0_pending = s.resp0_pending;
        self.resp1_delay = s.resp1_delay; self.resp1_pending = s.resp1_pending;
        self.resm0_delay = s.resm0_delay; self.resm0_pending = s.resm0_pending;
        self.resm1_delay = s.resm1_delay; self.resm1_pending = s.resm1_pending;
        self.resbl_delay = s.resbl_delay; self.resbl_pending = s.resbl_pending;
        self.inpt4 = s.inpt4; self.inpt5 = s.inpt5;
        self.collision = s.collision;
        self.scanline = s.scanline; self.clock = s.clock;
        self.wsync = s.wsync; self.vsync = s.vsync; self.vblank = s.vblank; self.frame_ready = s.frame_ready;
        rc(&mut self.audio_ch[0], &s.audio_ch[0]);
        rc(&mut self.audio_ch[1], &s.audio_ch[1]);
    }
}
```

**Note:** `hmove_pending`, `hmove_blanking`, and the `*_delay`/`*_pending` position pipeline fields are private. You need to either change them to `pub(crate)` in `tia/mod.rs` or add the snapshot/restore directly to the `Tia` impl (which can access private fields). Since the snapshot code lives in `tia/mod.rs`, private access is fine.

**Step 3: Add snapshot/restore to RIOT**

In `crates/atari2600/src/riot.rs`, add:

```rust
use crate::snapshot::RiotSnapshot;

impl Riot {
    pub fn snapshot(&self) -> RiotSnapshot {
        RiotSnapshot {
            ram: self.ram,
            swcha: self.swcha, swcha_out: self.swcha_out, swacnt: self.swacnt,
            swchb: self.swchb, swbcnt: self.swbcnt,
            timer_value: self.timer_value,
            timer_interval: self.timer_interval,
            timer_subcycles: self.timer_subcycles,
            timer_expired: self.timer_expired,
            timer_flag: self.timer_flag,
        }
    }

    pub fn restore(&mut self, s: &RiotSnapshot) {
        self.ram = s.ram;
        self.swcha = s.swcha; self.swcha_out = s.swcha_out; self.swacnt = s.swacnt;
        self.swchb = s.swchb; self.swbcnt = s.swbcnt;
        self.timer_value = s.timer_value;
        self.timer_interval = s.timer_interval;
        self.timer_subcycles = s.timer_subcycles;
        self.timer_expired = s.timer_expired;
        self.timer_flag = s.timer_flag;
    }
}
```

**Step 4: Implement `save_state` / `load_state` on `Atari2600` in `lib.rs`**

```rust
// At top of file:
use crate::snapshot::Atari2600Snapshot;

// In impl SystemEmulator for Atari2600:
fn supports_save_states(&self) -> bool { true }

fn save_state(&self) -> Result<Vec<u8>, String> {
    let snap = Atari2600Snapshot {
        cpu:  self.cpu.snapshot(),
        tia:  self.cpu.bus.tia.snapshot(),
        riot: self.cpu.bus.riot.snapshot(),
    };
    let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
        .map_err(|e| e.to_string())?;
    Ok(emu_common::save_encode(self.system_name(), &bytes))
}

fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let payload = emu_common::save_decode(self.system_name(), data)?;
    let (snap, _): (Atari2600Snapshot, _) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .map_err(|e| e.to_string())?;
    self.cpu.restore(&snap.cpu);
    self.cpu.bus.tia.restore(&snap.tia);
    self.cpu.bus.riot.restore(&snap.riot);
    Ok(())
}
```

**Step 5: Export snapshot module in `crates/atari2600/src/lib.rs`**

```rust
mod snapshot;
```

**Step 6: Build**

```
cargo build -p emu-atari2600
```
Expected: 0 warnings, 0 errors.

**Step 7: Commit**

```bash
git add crates/atari2600/src/snapshot.rs crates/atari2600/src/lib.rs \
        crates/atari2600/src/tia/mod.rs crates/atari2600/src/riot.rs
git commit -m "feat(atari2600): implement save/load state"
```

---

## Task 5: NES save states

**Files:**
- Create: `crates/nes/src/snapshot.rs`
- Modify: `crates/nes/src/cartridge/mapper/mod.rs`
- Modify: `crates/nes/src/ppu/mod.rs`
- Modify: `crates/nes/src/apu/mod.rs`
- Modify: `crates/nes/src/apu/pulse.rs`
- Modify: `crates/nes/src/apu/triangle.rs`
- Modify: `crates/nes/src/apu/noise.rs`
- Modify: `crates/nes/src/apu/dmc.rs`
- Modify: `crates/nes/src/bus.rs`
- Modify: `crates/nes/src/nes.rs`

**Context:** NES struct is in `crates/nes/src/nes.rs`. Bus: `NesBus` in `bus.rs` with `ram: [u8; 2048]`, `ppu: Ppu`, `apu: Apu`, `cartridge: Cartridge`. Mapper is `Box<dyn Mapper>` — add opaque `mapper_state()`/`restore_mapper_state()` to the trait. System name: `"NES"`.

### Step 1: Add mapper state to `Mapper` trait

In `crates/nes/src/cartridge/mapper/mod.rs`:

```rust
/// Serialize mapper-specific bank register state.
/// Default: return empty vec (ROM-only mappers like NROM have no state).
fn mapper_state(&self) -> Vec<u8> { vec![] }

/// Restore mapper state from bytes previously returned by `mapper_state`.
fn restore_mapper_state(&mut self, data: &[u8]) { let _ = data; }
```

Then implement for each stateful mapper. For mappers that use only a few register bytes, serialize manually. Example for MMC1 (`crates/nes/src/cartridge/mapper/mmc1.rs`):

```rust
fn mapper_state(&self) -> Vec<u8> {
    // Read the actual MMC1 struct fields — shift_register, shift_count, control,
    // chr_bank_0, chr_bank_1, prg_bank, prg_mode, chr_mode
    vec![self.shift_register, self.shift_count, self.control,
         self.chr_bank_0, self.chr_bank_1, self.prg_bank,
         self.prg_mode, self.chr_mode]
}
fn restore_mapper_state(&mut self, data: &[u8]) {
    if data.len() >= 8 {
        self.shift_register = data[0]; self.shift_count = data[1];
        self.control = data[2];
        self.chr_bank_0 = data[3]; self.chr_bank_1 = data[4];
        self.prg_bank = data[5]; self.prg_mode = data[6]; self.chr_mode = data[7];
    }
}
```

Do the same pattern for UxROM (1 byte: prg_bank), CNROM (1 byte: chr_bank), MMC3 (read the struct fields and serialize them all), etc. Check each mapper file for its struct fields.

### Step 2: Create `crates/nes/src/snapshot.rs`

```rust
use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct PpuSnapshot {
    pub nametable_ram: [u8; 2048],
    pub palette_ram: [u8; 32],
    pub oam: [u8; 256],
    pub ctrl: u8, pub mask: u8, pub status: u8, pub oam_addr: u8,
    pub v: u16, pub t: u16, pub fine_x: u8, pub w: bool,
    pub data_buffer: u8,
    pub scanline: i16, pub cycle: u16, pub frame_count: u64,
    pub nmi_pending: bool,
}

#[derive(Serialize, Deserialize)]
pub struct PulseSnapshot {
    pub duty: u8, pub duty_pos: u8,
    pub timer_period: u16, pub timer_counter: u16,
    pub length_counter: u8, pub length_halt: bool,
    pub envelope_start: bool, pub envelope_loop: bool, pub constant_volume: bool,
    pub envelope_period: u8, pub envelope_counter: u8, pub envelope_decay: u8,
    pub sweep_enabled: bool, pub sweep_period: u8, pub sweep_negate: bool,
    pub sweep_shift: u8, pub sweep_counter: u8, pub sweep_reload: bool,
    pub is_pulse1: bool,
}

#[derive(Serialize, Deserialize)]
pub struct TriangleSnapshot {
    pub timer_period: u16, pub timer_counter: u16,
    pub sequence_pos: u8,
    pub length_counter: u8, pub length_halt: bool,
    pub linear_counter: u8, pub linear_reload_value: u8,
    pub linear_reload_flag: bool, pub control_flag: bool,
}

#[derive(Serialize, Deserialize)]
pub struct NoiseSnapshot {
    pub timer_period: u16, pub timer_counter: u16,
    pub shift: u16, pub mode: bool,
    pub length_counter: u8, pub length_halt: bool,
    pub envelope_start: bool, pub envelope_loop: bool, pub constant_volume: bool,
    pub envelope_period: u8, pub envelope_counter: u8, pub envelope_decay: u8,
}

#[derive(Serialize, Deserialize)]
pub struct DmcSnapshot {
    pub timer_period: u16, pub timer_counter: u16,
    pub output_level: u8,
    pub sample_addr: u16, pub sample_length: u16,
    pub current_addr: u16, pub bytes_remaining: u16,
    pub shift_register: u8, pub bits_remaining: u8,
    pub sample_buffer: Option<u8>, pub silence_flag: bool,
    pub irq_enabled: bool, pub loop_flag: bool, pub irq_pending: bool,
}

#[derive(Serialize, Deserialize)]
pub struct ApuSnapshot {
    pub pulse1: PulseSnapshot,
    pub pulse2: PulseSnapshot,
    pub triangle: TriangleSnapshot,
    pub noise: NoiseSnapshot,
    pub dmc: DmcSnapshot,
    pub frame_counter_mode: u8,
    pub frame_counter: u32,
    pub frame_irq_inhibit: bool,
    pub frame_irq_pending: bool,
    pub enabled: [bool; 5],
}

#[derive(Serialize, Deserialize)]
pub struct NesSnapshot {
    pub cpu: Cpu6502Snapshot,
    pub ram: [u8; 2048],
    pub ppu: PpuSnapshot,
    pub apu: ApuSnapshot,
    pub mapper_state: Vec<u8>,
}
```

### Step 3: Add snapshot/restore to PPU, APU channels, and APU

For each hardware struct, add `pub fn snapshot(&self) -> XxxSnapshot` and `pub fn restore(&mut self, s: &XxxSnapshot)` methods. The pattern is identical to the Atari 2600 approach — copy each field. These methods go in the respective files.

**PPU** (`crates/nes/src/ppu/mod.rs`): snapshot all fields listed in `PpuSnapshot`; set `frame_ready: false` in the snapshot (don't carry over a stale ready flag). The sprite scanline buffers and rendering buffers are transient — exclude them.

**Pulse** (`crates/nes/src/apu/pulse.rs`): all `PulseSnapshot` fields.

**Triangle** (`crates/nes/src/apu/triangle.rs`): all `TriangleSnapshot` fields.

**Noise** (`crates/nes/src/apu/noise.rs`): all `NoiseSnapshot` fields.

**DMC** (`crates/nes/src/apu/dmc.rs`): all `DmcSnapshot` fields.

**APU** (`crates/nes/src/apu/mod.rs`):
```rust
pub fn snapshot(&self) -> ApuSnapshot {
    ApuSnapshot {
        pulse1: self.pulse1.snapshot(),
        pulse2: self.pulse2.snapshot(),
        triangle: self.triangle.snapshot(),
        noise: self.noise.snapshot(),
        dmc: self.dmc.snapshot(),
        frame_counter_mode: self.frame_counter_mode,
        frame_counter: self.frame_counter,
        frame_irq_inhibit: self.frame_irq_inhibit,
        frame_irq_pending: self.frame_irq_pending,
        enabled: self.enabled,
    }
}
pub fn restore(&mut self, s: &ApuSnapshot) {
    self.pulse1.restore(&s.pulse1);
    self.pulse2.restore(&s.pulse2);
    self.triangle.restore(&s.triangle);
    self.noise.restore(&s.noise);
    self.dmc.restore(&s.dmc);
    self.frame_counter_mode = s.frame_counter_mode;
    self.frame_counter = s.frame_counter;
    self.frame_irq_inhibit = s.frame_irq_inhibit;
    self.frame_irq_pending = s.frame_irq_pending;
    self.enabled = s.enabled;
}
```

### Step 4: Implement `save_state` / `load_state` on `Nes`

In `crates/nes/src/nes.rs`:

```rust
fn supports_save_states(&self) -> bool { true }

fn save_state(&self) -> Result<Vec<u8>, String> {
    let snap = NesSnapshot {
        cpu:          self.cpu.snapshot(),
        ram:          self.cpu.bus.ram,
        ppu:          self.cpu.bus.ppu.snapshot(),
        apu:          self.cpu.bus.apu.snapshot(),
        mapper_state: self.cpu.bus.cartridge.mapper.mapper_state(),
    };
    let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
        .map_err(|e| e.to_string())?;
    Ok(emu_common::save_encode(self.system_name(), &bytes))
}

fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let payload = emu_common::save_decode(self.system_name(), data)?;
    let (snap, _): (NesSnapshot, _) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .map_err(|e| e.to_string())?;
    self.cpu.restore(&snap.cpu);
    self.cpu.bus.ram = snap.ram;
    self.cpu.bus.ppu.restore(&snap.ppu);
    self.cpu.bus.apu.restore(&snap.apu);
    self.cpu.bus.cartridge.mapper.restore_mapper_state(&snap.mapper_state);
    Ok(())
}
```

### Step 5: Export snapshot module

In `crates/nes/src/lib.rs`:
```rust
mod snapshot;
```

### Step 6: Build

```
cargo build -p emu-nes
```
Expected: 0 warnings, 0 errors.

### Step 7: Commit

```bash
git add crates/nes/src/snapshot.rs crates/nes/src/nes.rs \
        crates/nes/src/ppu/mod.rs crates/nes/src/apu/ \
        crates/nes/src/cartridge/mapper/
git commit -m "feat(nes): implement save/load state"
```

---

## Task 6: Apple II save states

**Files:**
- Create: `crates/apple2/src/snapshot.rs`
- Modify: `crates/apple2/src/memory.rs`
- Modify: `crates/apple2/src/speaker.rs`
- Modify: `crates/apple2/src/lib.rs`

**Context:** Apple2 has `Cpu6502<Apple2Bus>`. Bus owns: `memory: Memory`, `switches: SoftSwitches`, `keyboard: Keyboard`, `speaker: Speaker`, `disk_ii: DiskII`. Exclude `disk_ii` (disk state is the separate `.dsk` file, not emulator state), `framebuffer`, and audio sample buffers. System name: `"Apple2"`.

**Step 1: Create `crates/apple2/src/snapshot.rs`**

```rust
use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;
use crate::soft_switch::SoftSwitches;

#[derive(Serialize, Deserialize)]
pub struct MemorySnapshot {
    pub ram: [u8; 49152],
    pub lc_ram: [u8; 16384],
    pub lc_bank2: [u8; 4096],
    pub lc_read_enable: bool,
    pub lc_write_enable: bool,
    pub lc_prewrite: bool,
    pub lc_bank1: bool,
    pub aux_ram: Vec<u8>,
    pub aux_lc_ram: Vec<u8>,
    pub aux_lc_bank2: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SoftSwitchSnapshot {
    pub text_mode: bool, pub mixed_mode: bool, pub page2: bool, pub hires: bool,
    pub an0: bool, pub an1: bool, pub an2: bool, pub an3: bool,
    pub store80: bool, pub ramrd: bool, pub ramwrt: bool, pub altzp: bool,
    pub intcxrom: bool, pub slotc3rom: bool, pub col80: bool, pub altcharset: bool,
    pub lc_bank2: bool, pub lc_read_enable: bool, pub vbl: bool,
    pub is_iie: bool, pub intc8rom: bool,
}

#[derive(Serialize, Deserialize)]
pub struct Apple2Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub memory: MemorySnapshot,
    pub switches: SoftSwitchSnapshot,
    pub keyboard_latch: u8,
    pub keyboard_strobe: bool,
    pub speaker_state: bool,
    pub speaker_active: bool,
    pub speaker_cycles_since_toggle: u64,
    pub bus_cycle_count: u64,
}
```

**Step 2: Add snapshot/restore to `Memory`, `SoftSwitches`, `Speaker`**

`Memory::snapshot()` — copies all fields listed above.
`Memory::restore()` — restores all fields; leaves `rom` untouched (ROM is loaded from disk).
`SoftSwitches::snapshot()` / `restore()` — copies all bool fields.
`Speaker::snapshot()` — returns (state, active, cycles_since_toggle). Add an `Apple2Snapshot`-level helper instead of a separate speaker snapshot struct to keep it simple.

**Step 3: Implement `save_state` / `load_state` on `Apple2`**

```rust
fn supports_save_states(&self) -> bool { true }

fn save_state(&self) -> Result<Vec<u8>, String> {
    let bus = &self.cpu.bus;
    let snap = Apple2Snapshot {
        cpu: self.cpu.snapshot(),
        memory: bus.memory.snapshot(),
        switches: bus.switches.snapshot(),
        keyboard_latch: bus.keyboard.latch,
        keyboard_strobe: bus.keyboard.strobe,
        speaker_state: bus.speaker.state,
        speaker_active: bus.speaker.active,
        speaker_cycles_since_toggle: bus.speaker.cycles_since_toggle,
        bus_cycle_count: bus.cycle_count,
    };
    let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
        .map_err(|e| e.to_string())?;
    Ok(emu_common::save_encode(self.system_name(), &bytes))
}

fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let payload = emu_common::save_decode(self.system_name(), data)?;
    let (snap, _): (Apple2Snapshot, _) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .map_err(|e| e.to_string())?;
    let bus = &mut self.cpu.bus;
    self.cpu.restore(&snap.cpu);  // Note: need to call on cpu, not bus
    bus.memory.restore(&snap.memory);
    bus.switches.restore(&snap.switches);
    bus.keyboard.latch = snap.keyboard_latch;
    bus.keyboard.strobe = snap.keyboard_strobe;
    bus.speaker.state = snap.speaker_state;
    bus.speaker.active = snap.speaker_active;
    bus.speaker.cycles_since_toggle = snap.speaker_cycles_since_toggle;
    bus.cycle_count = snap.bus_cycle_count;
    Ok(())
}
```

**Note:** `self.cpu.restore(&snap.cpu)` borrows `self.cpu` while `self.cpu.bus` is borrowed. To avoid the borrow conflict, restore CPU before borrowing bus, or use a temporary. Pattern: let bus = &mut self.cpu.bus; self.cpu.restore() is a conflict — fix by restoring CPU first, then reborrowing bus:

```rust
fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let payload = emu_common::save_decode(self.system_name(), data)?;
    let (snap, _): (Apple2Snapshot, _) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .map_err(|e| e.to_string())?;
    self.cpu.restore(&snap.cpu);
    self.cpu.bus.memory.restore(&snap.memory);
    self.cpu.bus.switches.restore(&snap.switches);
    self.cpu.bus.keyboard.latch = snap.keyboard_latch;
    self.cpu.bus.keyboard.strobe = snap.keyboard_strobe;
    self.cpu.bus.speaker.state = snap.speaker_state;
    self.cpu.bus.speaker.active = snap.speaker_active;
    self.cpu.bus.speaker.cycles_since_toggle = snap.speaker_cycles_since_toggle;
    self.cpu.bus.cycle_count = snap.bus_cycle_count;
    Ok(())
}
```

`Speaker`'s `state`, `active`, `cycles_since_toggle` are private — make them `pub(crate)` in `speaker.rs`.

**Step 4: Export snapshot module**

```rust
// crates/apple2/src/lib.rs
mod snapshot;
```

**Step 5: Build**

```
cargo build -p emu-apple2
```
Expected: 0 warnings, 0 errors.

**Step 6: Commit**

```bash
git add crates/apple2/src/snapshot.rs crates/apple2/src/lib.rs \
        crates/apple2/src/memory.rs crates/apple2/src/speaker.rs
git commit -m "feat(apple2): implement save/load state"
```

---

## Task 7: C64 save states

**Files:**
- Create: `crates/c64/src/snapshot.rs`
- Modify: `crates/c64/src/vic_ii/mod.rs`
- Modify: `crates/c64/src/sid/mod.rs`
- Modify: `crates/c64/src/cia/mod.rs`
- Modify: `crates/c64/src/lib.rs`

**Context:** C64 has `Cpu6502<C64Bus>`. Bus: `memory: Memory`, `vic: VicII`, `sid: Sid`, `cia1: Cia`, `cia2: Cia`. Drive 1541 is excluded. System name: `"C64"`.

**Step 1: Create `crates/c64/src/snapshot.rs`**

```rust
use serde::{Serialize, Deserialize};
use emu_cpu::Cpu6502Snapshot;

#[derive(Serialize, Deserialize)]
pub struct VoiceSnapshot {
    pub frequency: u16, pub pulse_width: u16, pub control: u8,
    pub attack: u8, pub decay: u8, pub sustain: u8, pub release: u8,
    pub gate: bool,
    pub accumulator: u32, pub prev_msb: bool, pub noise_lfsr: u32,
    pub envelope: u8,
    pub envelope_state: u8,   // 0=Attack,1=Decay,2=Sustain,3=Release
    pub envelope_counter: u32,
}

#[derive(Serialize, Deserialize)]
pub struct SidSnapshot {
    pub voices: [VoiceSnapshot; 3],
    pub filter_cutoff: u16, pub filter_resonance: u8,
    pub filter_mode: u8, pub filter_routing: u8, pub voice3_off: bool, pub volume: u8,
    pub filter_bp: f32, pub filter_lp: f32,
}

#[derive(Serialize, Deserialize)]
pub struct VicSnapshot {
    pub registers: [u8; 64],
    pub raster_line: u16, pub raster_irq_line: u16, pub cycle: u16,
    pub irq_pending: bool,
    pub color_ram: [u8; 1024],
    pub vic_bank_base: u16,
    pub sprite_sprite_collision: u8, pub sprite_bg_collision: u8,
    pub stall_cycles: u8,
}

#[derive(Serialize, Deserialize)]
pub struct CiaSnapshot {
    pub pra: u8, pub prb: u8, pub ddra: u8, pub ddrb: u8,
    pub timer_a_latch: u16, pub timer_a_counter: u16,
    pub timer_a_running: bool, pub timer_a_oneshot: bool,
    pub timer_b_latch: u16, pub timer_b_counter: u16,
    pub timer_b_running: bool, pub timer_b_oneshot: bool,
    pub icr_data: u8, pub icr_mask: u8, pub irq_pending: bool,
    pub is_cia1: bool,
    pub keyboard_matrix: [u8; 8],
    pub joy2: u8,
}

#[derive(Serialize, Deserialize)]
pub struct C64Snapshot {
    pub cpu: Cpu6502Snapshot,
    pub ram: Box<[u8; 65536]>,
    pub cpu_port: u8, pub cpu_port_dir: u8,
    pub vic: VicSnapshot,
    pub sid: SidSnapshot,
    pub cia1: CiaSnapshot,
    pub cia2: CiaSnapshot,
}
```

**Step 2: Add snapshot/restore to VicII**

In `crates/c64/src/vic_ii/mod.rs`, the private fields `raster_line`, `raster_irq_line`, `cycle`, `frame_ready`, `sprite_sprite_collision`, `sprite_bg_collision`, `stall_cycles` need to be snapshotted from within the `VicII` impl:

```rust
use crate::snapshot::VicSnapshot;

impl VicII {
    pub fn snapshot(&self) -> VicSnapshot {
        VicSnapshot {
            registers: self.registers,
            raster_line: self.raster_line, raster_irq_line: self.raster_irq_line,
            cycle: self.cycle, irq_pending: self.irq_pending,
            color_ram: self.color_ram, vic_bank_base: self.vic_bank_base,
            sprite_sprite_collision: self.sprite_sprite_collision,
            sprite_bg_collision: self.sprite_bg_collision,
            stall_cycles: self.stall_cycles,
        }
    }
    pub fn restore(&mut self, s: &VicSnapshot) {
        self.registers = s.registers;
        self.raster_line = s.raster_line; self.raster_irq_line = s.raster_irq_line;
        self.cycle = s.cycle; self.irq_pending = s.irq_pending;
        self.color_ram = s.color_ram; self.vic_bank_base = s.vic_bank_base;
        self.sprite_sprite_collision = s.sprite_sprite_collision;
        self.sprite_bg_collision = s.sprite_bg_collision;
        self.stall_cycles = s.stall_cycles;
    }
}
```

**Step 3: Add snapshot/restore to SID**

`EnvelopeState` is private and non-serializable. Convert to/from `u8` (0=Attack,1=Decay,2=Sustain,3=Release) in the snapshot methods. `voice3_off`, `filter_bp`, `filter_lp` are private — add methods from within `sid/mod.rs`.

```rust
use crate::snapshot::{SidSnapshot, VoiceSnapshot};

impl Sid {
    pub fn snapshot(&self) -> SidSnapshot {
        let vs = |v: &Voice| VoiceSnapshot {
            frequency: v.frequency, pulse_width: v.pulse_width, control: v.control,
            attack: v.attack, decay: v.decay, sustain: v.sustain, release: v.release,
            gate: v.gate,
            accumulator: v.accumulator, prev_msb: v.prev_msb, noise_lfsr: v.noise_lfsr,
            envelope: v.envelope,
            envelope_state: match v.envelope_state {
                EnvelopeState::Attack => 0, EnvelopeState::Decay => 1,
                EnvelopeState::Sustain => 2, EnvelopeState::Release => 3,
            },
            envelope_counter: v.envelope_counter,
        };
        SidSnapshot {
            voices: [vs(&self.voices[0]), vs(&self.voices[1]), vs(&self.voices[2])],
            filter_cutoff: self.filter_cutoff, filter_resonance: self.filter_resonance,
            filter_mode: self.filter_mode, filter_routing: self.filter_routing,
            voice3_off: self.voice3_off, volume: self.volume,
            filter_bp: self.filter_bp, filter_lp: self.filter_lp,
        }
    }

    pub fn restore(&mut self, s: &SidSnapshot) {
        let rv = |v: &mut Voice, sv: &VoiceSnapshot| {
            v.frequency = sv.frequency; v.pulse_width = sv.pulse_width; v.control = sv.control;
            v.attack = sv.attack; v.decay = sv.decay; v.sustain = sv.sustain; v.release = sv.release;
            v.gate = sv.gate;
            v.accumulator = sv.accumulator; v.prev_msb = sv.prev_msb; v.noise_lfsr = sv.noise_lfsr;
            v.envelope = sv.envelope;
            v.envelope_state = match sv.envelope_state {
                0 => EnvelopeState::Attack, 1 => EnvelopeState::Decay,
                2 => EnvelopeState::Sustain, _ => EnvelopeState::Release,
            };
            v.envelope_counter = sv.envelope_counter;
        };
        rv(&mut self.voices[0], &s.voices[0]);
        rv(&mut self.voices[1], &s.voices[1]);
        rv(&mut self.voices[2], &s.voices[2]);
        self.filter_cutoff = s.filter_cutoff; self.filter_resonance = s.filter_resonance;
        self.filter_mode = s.filter_mode; self.filter_routing = s.filter_routing;
        self.voice3_off = s.voice3_off; self.volume = s.volume;
        self.filter_bp = s.filter_bp; self.filter_lp = s.filter_lp;
    }
}
```

**Step 4: Add snapshot/restore to CIA**

In `crates/c64/src/cia/mod.rs`:

```rust
use crate::snapshot::CiaSnapshot;

impl Cia {
    pub fn snapshot(&self) -> CiaSnapshot {
        CiaSnapshot {
            pra: self.pra, prb: self.prb, ddra: self.ddra, ddrb: self.ddrb,
            timer_a_latch: self.timer_a_latch, timer_a_counter: self.timer_a_counter,
            timer_a_running: self.timer_a_running, timer_a_oneshot: self.timer_a_oneshot,
            timer_b_latch: self.timer_b_latch, timer_b_counter: self.timer_b_counter,
            timer_b_running: self.timer_b_running, timer_b_oneshot: self.timer_b_oneshot,
            icr_data: self.icr_data, icr_mask: self.icr_mask, irq_pending: self.irq_pending,
            is_cia1: self.is_cia1, keyboard_matrix: self.keyboard_matrix, joy2: self.joy2,
        }
    }
    pub fn restore(&mut self, s: &CiaSnapshot) {
        self.pra = s.pra; self.prb = s.prb; self.ddra = s.ddra; self.ddrb = s.ddrb;
        self.timer_a_latch = s.timer_a_latch; self.timer_a_counter = s.timer_a_counter;
        self.timer_a_running = s.timer_a_running; self.timer_a_oneshot = s.timer_a_oneshot;
        self.timer_b_latch = s.timer_b_latch; self.timer_b_counter = s.timer_b_counter;
        self.timer_b_running = s.timer_b_running; self.timer_b_oneshot = s.timer_b_oneshot;
        self.icr_data = s.icr_data; self.icr_mask = s.icr_mask; self.irq_pending = s.irq_pending;
        self.is_cia1 = s.is_cia1; self.keyboard_matrix = s.keyboard_matrix; self.joy2 = s.joy2;
    }
}
```

**Step 5: Implement `save_state` / `load_state` on `C64`**

In `crates/c64/src/lib.rs`:

```rust
fn supports_save_states(&self) -> bool { true }

fn save_state(&self) -> Result<Vec<u8>, String> {
    let bus = &self.cpu.bus;
    let snap = C64Snapshot {
        cpu:         self.cpu.snapshot(),
        ram:         Box::new(bus.memory.ram),
        cpu_port:    bus.memory.cpu_port,
        cpu_port_dir: bus.memory.cpu_port_dir,
        vic:         bus.vic.snapshot(),
        sid:         bus.sid.snapshot(),
        cia1:        bus.cia1.snapshot(),
        cia2:        bus.cia2.snapshot(),
    };
    let bytes = bincode::serde::encode_to_vec(&snap, bincode::config::standard())
        .map_err(|e| e.to_string())?;
    Ok(emu_common::save_encode(self.system_name(), &bytes))
}

fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
    let payload = emu_common::save_decode(self.system_name(), data)?;
    let (snap, _): (C64Snapshot, _) =
        bincode::serde::decode_from_slice(payload, bincode::config::standard())
            .map_err(|e| e.to_string())?;
    self.cpu.restore(&snap.cpu);
    self.cpu.bus.memory.ram = *snap.ram;
    self.cpu.bus.memory.cpu_port = snap.cpu_port;
    self.cpu.bus.memory.cpu_port_dir = snap.cpu_port_dir;
    self.cpu.bus.vic.restore(&snap.vic);
    self.cpu.bus.sid.restore(&snap.sid);
    self.cpu.bus.cia1.restore(&snap.cia1);
    self.cpu.bus.cia2.restore(&snap.cia2);
    Ok(())
}
```

**Step 6: Export snapshot module**

```rust
// crates/c64/src/lib.rs
mod snapshot;
```

**Step 7: Build**

```
cargo build -p emu-c64
```
Expected: 0 warnings, 0 errors.

**Step 8: Commit**

```bash
git add crates/c64/src/snapshot.rs crates/c64/src/lib.rs \
        crates/c64/src/vic_ii/mod.rs crates/c64/src/sid/mod.rs \
        crates/c64/src/cia/mod.rs
git commit -m "feat(c64): implement save/load state"
```

---

## Task 8: SaveManager (frontend file + manifest layer)

**Files:**
- Create: `crates/frontend/src/save_manager.rs`
- Modify: `crates/frontend/src/main.rs`

**Context:** `SaveManager` owns the file I/O and manifest for one ROM's save directory. It lives in the frontend crate only. Manifest format is RON.

**Step 1: Create `crates/frontend/src/save_manager.rs`**

```rust
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
        // Sanitize: keep alphanumeric, dash, underscore; truncate to 64.
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
        // Simple ISO-like timestamp from SystemTime (no chrono dep).
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Format as YYYY-MM-DD HH:MM:SS UTC (approximate, no timezone library).
        let s = secs;
        let sec = s % 60; let s = s / 60;
        let min = s % 60; let s = s / 60;
        let hour = s % 24; let days = s / 24;
        // Approximate date from epoch (good enough for display).
        let year = 1970 + days / 365;
        let day_of_year = days % 365;
        let month = day_of_year / 30 + 1;
        let day = day_of_year % 30 + 1;
        format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", year, month, day, hour, min, sec)
    }

    /// Save to a numbered slot (1-8). Creates/overwrites the state file.
    pub fn save_to_slot(&mut self, slot: u8, name: &str, data: &[u8]) -> Result<(), String> {
        let filename = format!("slot{}.state", slot);
        let path = self.dir.join(&filename);
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        let entry = SlotEntry {
            filename: filename.clone(),
            name: if name.is_empty() { format!("Slot {}", slot) } else { name.to_string() },
            saved_at: Self::timestamp(),
        };
        // Also upsert into named list so Browse shows it.
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

    /// Save to a new named file (not assigned to any slot).
    pub fn save_named(&mut self, name: &str, data: &[u8]) -> Result<(), String> {
        // Generate a safe filename from the name.
        let safe: String = name.chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .take(48)
            .collect();
        let ts = Self::timestamp().replace([':', ' ', '-'], "");
        let filename = format!("named_{}_{}.state", safe, &ts[..8]);
        let path = self.dir.join(&filename);
        std::fs::write(&path, data).map_err(|e| e.to_string())?;
        self.manifest.named.push(NamedEntry {
            filename,
            name: name.to_string(),
            saved_at: Self::timestamp(),
            slot: None,
        });
        self.save_manifest();
        Ok(())
    }

    /// Load bytes from a slot. Returns Err if slot is empty.
    pub fn load_slot(&self, slot: u8) -> Result<Vec<u8>, String> {
        let entry = self.manifest.slots.get(&slot)
            .ok_or_else(|| format!("Slot {} is empty", slot))?;
        let path = self.dir.join(&entry.filename);
        std::fs::read(&path).map_err(|e| e.to_string())
    }

    /// Load bytes from a named entry by filename.
    pub fn load_named(&self, filename: &str) -> Result<Vec<u8>, String> {
        let path = self.dir.join(filename);
        std::fs::read(&path).map_err(|e| e.to_string())
    }

    /// Assign an existing named save to a slot.
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
        // Clear old slot assignment for any other entry pointing to this slot.
        for ne in &mut self.manifest.named {
            if ne.slot == Some(slot) && ne.filename != filename {
                ne.slot = None;
            }
        }
        self.manifest.slots.insert(slot, slot_entry);
        self.save_manifest();
        Ok(())
    }

    /// Delete a named save by filename.
    pub fn delete_named(&mut self, filename: &str) -> Result<(), String> {
        let path = self.dir.join(filename);
        let _ = std::fs::remove_file(&path);
        // Remove from slots map if assigned.
        self.manifest.slots.retain(|_, v| v.filename != filename);
        self.manifest.named.retain(|n| n.filename != filename);
        self.save_manifest();
        Ok(())
    }

    pub fn slot_info(&self, slot: u8) -> Option<&SlotEntry> {
        self.manifest.slots.get(&slot)
    }

    pub fn all_named(&self) -> &[NamedEntry] {
        &self.manifest.named
    }
}
```

**Step 2: Add mod to `crates/frontend/src/main.rs`**

```rust
mod save_manager;
```

**Step 3: Build**

```
cargo build -p emu-frontend
```
Expected: 0 warnings, 0 errors.

**Step 4: Commit**

```bash
git add crates/frontend/src/save_manager.rs crates/frontend/src/main.rs
git commit -m "feat(frontend): add SaveManager with slot/manifest system"
```

---

## Task 9: Wire SaveManager into EmuApp + Config

**Files:**
- Modify: `crates/frontend/src/config.rs`
- Modify: `crates/frontend/src/app.rs`

**Step 1: Add `saves_dir` to `Config`**

In `crates/frontend/src/config.rs`, add to `Config` struct:
```rust
pub saves_dir: String,
```
Add to `Default`:
```rust
saves_dir: "saves".into(),
```

**Step 2: Add fields to `EmuApp`**

In `crates/frontend/src/app.rs`:

```rust
use crate::save_manager::SaveManager;
// ...

pub struct EmuApp {
    // ... existing fields ...
    save_manager: Option<SaveManager>,
    save_name_input: String,
    show_save_dialog: bool,
    show_browse_saves: bool,
    pending_load_slot: Option<u8>,         // deferred load after menu closes
    pending_load_named: Option<String>,    // deferred load by filename
}
```

In `EmuApp::new()`:
```rust
save_manager: None,
save_name_input: String::new(),
show_save_dialog: false,
show_browse_saves: false,
pending_load_slot: None,
pending_load_named: None,
```

**Step 3: Create `SaveManager` in `start_system()`**

In `start_system()`, after `self.system = Some(sys)`:

```rust
// Create save manager if we have a ROM path context.
// Note: start_system doesn't receive the ROM path directly.
// Store rom_path as a field, or pass it in.
```

**Change of approach:** `start_system()` doesn't know the ROM path. Pass it explicitly. Modify `start_system()` signature:

```rust
fn start_system(&mut self, sys: Box<dyn SystemEmulator>, rom_path: Option<&std::path::Path>)
```

And at the end of `start_system()`:

```rust
let saves_root = std::path::PathBuf::from(&self.config.saves_dir);
self.save_manager = rom_path.map(|p| {
    SaveManager::new(
        &saves_root,
        sys.system_name(),
        p,
    )
});
// Note: `sys` has been moved into self.system above.
// Use self.system.as_ref().unwrap().system_name() for the name.
```

Update all callers of `start_system()` to pass `Some(path)` when a ROM was loaded or `None` for boot-from-ROM-only (like C64 BASIC boot). The callers are `boot_system()` and `load_rom()`.

For `boot_system()`: pass `None` (no ROM file path).
For `load_rom()`: pass `Some(&path)` after file pick.

**Step 4: Handle pending deferred save/load in `update()`**

At the start of `update()`, before rendering, process deferred operations:

```rust
// Process deferred save state operations (deferred because menu closes after click)
if let Some(slot) = self.pending_load_slot.take() {
    self.do_load_slot(slot);
}
if let Some(filename) = self.pending_load_named.take() {
    self.do_load_named(filename);
}
```

Add private helper methods:

```rust
fn do_save_slot(&mut self, slot: u8, name: &str) {
    let Some(ref sys) = self.system else { return };
    let Some(ref mut sm) = self.save_manager else {
        self.error_msg = Some("No ROM loaded, cannot save state".into());
        return;
    };
    match sys.save_state() {
        Ok(data) => {
            if let Err(e) = sm.save_to_slot(slot, name, &data) {
                self.error_msg = Some(format!("Save failed: {}", e));
            }
        }
        Err(e) => self.error_msg = Some(format!("Save state error: {}", e)),
    }
}

fn do_load_slot(&mut self, slot: u8) {
    let data = {
        let Some(ref sm) = self.save_manager else { return };
        match sm.load_slot(slot) {
            Ok(d) => d,
            Err(e) => { self.error_msg = Some(format!("Load failed: {}", e)); return; }
        }
    };
    if let Some(ref mut sys) = self.system {
        if let Err(e) = sys.load_state(&data) {
            self.error_msg = Some(format!("Load state error: {}", e));
        }
    }
}

fn do_load_named(&mut self, filename: String) {
    let data = {
        let Some(ref sm) = self.save_manager else { return };
        match sm.load_named(&filename) {
            Ok(d) => d,
            Err(e) => { self.error_msg = Some(format!("Load failed: {}", e)); return; }
        }
    };
    if let Some(ref mut sys) = self.system {
        if let Err(e) = sys.load_state(&data) {
            self.error_msg = Some(format!("Load state error: {}", e));
        }
    }
}
```

**Step 5: Build**

```
cargo build --workspace
```
Expected: 0 warnings, 0 errors.

**Step 6: Commit**

```bash
git add crates/frontend/src/app.rs crates/frontend/src/config.rs
git commit -m "feat(frontend): wire SaveManager into EmuApp"
```

---

## Task 10: Menu UI — Save/Load State submenus

**Files:**
- Modify: `crates/frontend/src/menu.rs`
- Modify: `crates/frontend/src/app.rs`

**Context:** Menu needs to show slots 1–8 with names/dates. Slot info comes from `SaveManager`.

**Step 1: Add new `MenuAction` variants in `menu.rs`**

```rust
pub enum MenuAction {
    None,
    LoadRom,
    Reset,
    Break,
    Quit,
    BackToSystemSelect,
    SetCrtMode(CrtMode),
    ToggleDebugger,
    SaveToSlot(u8),          // save to slot N
    LoadFromSlot(u8),        // load from slot N
    SaveNamed,               // open "save to new named" dialog
    BrowseSaves,             // open browse window
}
```

**Step 2: Add slot info parameter to `render_menu()`**

```rust
pub fn render_menu(
    ui: &mut Ui,
    has_system: bool,
    crt_mode: CrtMode,
    save_slots: Option<&[Option<(String, String)>; 8]>, // [name, date] per slot, None if empty
    supports_saves: bool,
) -> MenuAction
```

`save_slots` is `None` if no save manager exists; `Some(array)` otherwise.

**Step 3: Add Save State and Load State submenus to System menu**

```rust
if has_system {
    ui.menu_button("System", |ui| {
        // ... Reset, Break ...

        if supports_saves {
            ui.separator();
            ui.menu_button("Save State", |ui| {
                for slot in 1u8..=8 {
                    let label = if let Some(Some((name, _date))) = save_slots.map(|s| &s[(slot-1) as usize]) {
                        format!("[{}] {}", slot, name)
                    } else {
                        format!("[{}] (empty)", slot)
                    };
                    if ui.button(&label).clicked() {
                        action = MenuAction::SaveToSlot(slot);
                        ui.close_menu();
                    }
                }
                ui.separator();
                if ui.button("Save to new named...").clicked() {
                    action = MenuAction::SaveNamed;
                    ui.close_menu();
                }
            });
            ui.menu_button("Load State", |ui| {
                for slot in 1u8..=8 {
                    let info = save_slots.and_then(|s| s[(slot-1) as usize].as_ref());
                    if let Some((name, date)) = info {
                        if ui.button(format!("[{}] {}  {}", slot, name, date)).clicked() {
                            action = MenuAction::LoadFromSlot(slot);
                            ui.close_menu();
                        }
                    } else {
                        ui.add_enabled(false, egui::Button::new(format!("[{}] (empty)", slot)));
                    }
                }
                ui.separator();
                if ui.button("Browse all saves...").clicked() {
                    action = MenuAction::BrowseSaves;
                    ui.close_menu();
                }
            });
        } else {
            ui.add_enabled(false, egui::Button::new("Save State"));
            ui.add_enabled(false, egui::Button::new("Load State"));
        }

        ui.separator();
        // ... Change System ...
    });
}
```

**Step 4: Wire new actions in `app.rs` `update()`**

Build the slot array before calling `render_menu`:

```rust
let supports_saves = self.system.as_ref().map_or(false, |s| s.supports_save_states());
let save_slots: Option<[Option<(String, String)>; 8]> = self.save_manager.as_ref().map(|sm| {
    std::array::from_fn(|i| {
        sm.slot_info(i as u8 + 1).map(|e| (e.name.clone(), e.saved_at.clone()))
    })
});
let action = menu::render_menu(ui, self.system.is_some(), self.config.crt_mode,
    save_slots.as_ref(), supports_saves);
```

Handle new actions in the match:

```rust
MenuAction::SaveToSlot(slot) => {
    self.do_save_slot(slot, "");
}
MenuAction::LoadFromSlot(slot) => {
    self.pending_load_slot = Some(slot);
}
MenuAction::SaveNamed => {
    self.show_save_dialog = true;
}
MenuAction::BrowseSaves => {
    self.show_browse_saves = true;
}
```

**Step 5: Build**

```
cargo build --workspace
```
Expected: 0 warnings, 0 errors.

**Step 6: Commit**

```bash
git add crates/frontend/src/menu.rs crates/frontend/src/app.rs
git commit -m "feat(frontend): add Save/Load State submenus"
```

---

## Task 11: Save dialog and Browse saves window

**Files:**
- Modify: `crates/frontend/src/app.rs`

**Context:** Both dialogs are simple `egui::Window`s rendered in `update()` after the main panels.

**Step 1: Add "Save to new named" dialog**

In `update()`, after the error message window:

```rust
// "Save to new named" dialog
if self.show_save_dialog {
    egui::Window::new("Save State As")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Save name:");
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.save_name_input)
                    .desired_width(200.0)
                    .hint_text("e.g. Before Boss"),
            );
            resp.request_focus();
            ui.horizontal(|ui| {
                let can_save = !self.save_name_input.trim().is_empty();
                if ui.add_enabled(can_save, egui::Button::new("Save")).clicked()
                    || (can_save && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let name = self.save_name_input.trim().to_string();
                    self.save_name_input.clear();
                    self.show_save_dialog = false;
                    // Perform the save
                    if let (Some(ref sys), Some(ref mut sm)) = (&self.system, &mut self.save_manager) {
                        match sys.save_state() {
                            Ok(data) => {
                                if let Err(e) = sm.save_named(&name, &data) {
                                    self.error_msg = Some(format!("Save failed: {}", e));
                                }
                            }
                            Err(e) => self.error_msg = Some(format!("Save error: {}", e)),
                        }
                    }
                }
                if ui.button("Cancel").clicked() {
                    self.show_save_dialog = false;
                    self.save_name_input.clear();
                }
            });
        });
}
```

**Step 2: Add "Browse saves" window**

```rust
// Browse saves window
if self.show_browse_saves {
    if let Some(ref mut sm) = self.save_manager {
        let named = sm.manifest.named.clone();
        let mut pending_load: Option<String> = None;
        let mut pending_delete: Option<String> = None;
        let mut pending_assign: Option<(String, u8)> = None;

        egui::Window::new("Browse Saves")
            .collapsible(false)
            .resizable(true)
            .default_size([480.0, 300.0])
            .show(ctx, |ui| {
                if named.is_empty() {
                    ui.label("No saves yet.");
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &named {
                        ui.horizontal(|ui| {
                            ui.label(&entry.name);
                            ui.weak(&entry.saved_at);
                            if let Some(slot) = entry.slot {
                                ui.label(egui::RichText::new(format!("Slot {}", slot)).weak());
                            }
                            if ui.small_button("Load").clicked() {
                                pending_load = Some(entry.filename.clone());
                                self.show_browse_saves = false;
                            }
                            // Assign to slot popup
                            ui.menu_button("Assign", |ui| {
                                for s in 1u8..=8 {
                                    if ui.button(format!("Slot {}", s)).clicked() {
                                        pending_assign = Some((entry.filename.clone(), s));
                                        ui.close_menu();
                                    }
                                }
                            });
                            if ui.small_button("Delete").clicked() {
                                pending_delete = Some(entry.filename.clone());
                            }
                        });
                    }
                });
                ui.separator();
                if ui.button("Close").clicked() {
                    self.show_browse_saves = false;
                }
            });

        // Apply pending actions after the borrow on sm is released
        if let Some(filename) = pending_load {
            self.pending_load_named = Some(filename);
        }
        if let Some(filename) = pending_delete {
            if let Some(ref mut sm) = self.save_manager {
                let _ = sm.delete_named(&filename);
            }
        }
        if let Some((filename, slot)) = pending_assign {
            if let Some(ref mut sm) = self.save_manager {
                let _ = sm.assign_to_slot(&filename, slot);
            }
        }
    }
}
```

**Step 3: Build and smoke test**

```
cargo build --workspace
```
Expected: 0 warnings, 0 errors.

Run the emulator, load a NES ROM, use System → Save State → [1] to save, then load. Verify state is restored correctly.

**Step 4: Commit**

```bash
git add crates/frontend/src/app.rs
git commit -m "feat(frontend): add save-to-named dialog and browse saves window"
```

---

## Final verification

```
cargo test --workspace
cargo build --workspace
```

Both must pass with 0 warnings, 0 errors.
