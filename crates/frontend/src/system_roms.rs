use std::path::{Path, PathBuf};

/// Try to read a file, returning None if it doesn't exist or can't be read.
fn try_load(path: &Path) -> Option<Vec<u8>> {
    match std::fs::read(path) {
        Ok(data) => {
            log::info!("Loaded system ROM: {}", path.display());
            Some(data)
        }
        Err(_) => {
            log::debug!("System ROM not found: {}", path.display());
            None
        }
    }
}

/// Expected system ROM filenames for each system.
/// The loader checks for each name in order, using the first match.

/// C64 system ROM file names (checked in order of preference).
const C64_BASIC_NAMES: &[&str] = &["basic.rom", "basic", "basic.bin", "901226-01.bin"];
const C64_KERNAL_NAMES: &[&str] = &["kernal.rom", "kernal", "kernal.bin", "901227-03.bin"];
const C64_CHARGEN_NAMES: &[&str] = &["chargen.rom", "chargen", "chargen.bin", "characters.rom", "901225-01.bin"];
const C64_1541_ROM_NAMES: &[&str] = &["1541.rom", "dos1541", "1541-II.rom"];
/// Split 1541 ROM pairs: (low $C000-$DFFF, high $E000-$FFFF).
const C64_1541_SPLIT_PAIRS: &[(&str, &str)] = &[
    ("325302-01.bin", "901229-05.bin"),
    ("1541-c000.bin", "1541-e000.bin"),
];

/// Apple II system ROM file names (checked in order of preference).
const APPLE2_ROM_NAMES: &[&str] = &[
    "apple2plus.rom", "apple2p.rom", "apple2.rom",
    "apple2p_.rom", "apple2_.rom",
    "apple2e.rom", "apple2e_enhanced.rom",
    "APPLE2.ROM", "Apple2plus.rom",
    "APPLE2P.ROM", "APPLE2E.ROM",
];

/// Load C64 system ROMs from the given directory.
/// Returns (basic, kernal, chargen, drive_1541) — each is Some if found.
pub fn load_c64_roms(dir: &Path) -> (Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>, Option<Vec<u8>>) {
    let subdir = dir.join("c64");

    let find = |names: &[&str]| -> Option<Vec<u8>> {
        for name in names {
            // Check c64/ subdirectory first, then root roms/ dir
            if let Some(data) = try_load(&subdir.join(name)) {
                return Some(data);
            }
            if let Some(data) = try_load(&dir.join(name)) {
                return Some(data);
            }
        }
        None
    };

    // Try single 16KB 1541 ROM first, then split 8KB+8KB pairs
    let drive_rom = find(C64_1541_ROM_NAMES).or_else(|| {
        for &(lo_name, hi_name) in C64_1541_SPLIT_PAIRS {
            let lo = try_load(&subdir.join(lo_name))
                .or_else(|| try_load(&dir.join(lo_name)));
            let hi = try_load(&subdir.join(hi_name))
                .or_else(|| try_load(&dir.join(hi_name)));
            if let (Some(lo_data), Some(hi_data)) = (lo, hi) {
                if lo_data.len() >= 8192 && hi_data.len() >= 8192 {
                    let mut combined = Vec::with_capacity(16384);
                    combined.extend_from_slice(&lo_data[..8192]);
                    combined.extend_from_slice(&hi_data[..8192]);
                    log::info!("Combined split 1541 ROMs: {} + {}", lo_name, hi_name);
                    return Some(combined);
                }
            }
        }
        None
    });

    (
        find(C64_BASIC_NAMES),
        find(C64_KERNAL_NAMES),
        find(C64_CHARGEN_NAMES),
        drive_rom,
    )
}

/// Load Apple II system ROM from the given directory.
pub fn load_apple2_rom(dir: &Path) -> Option<Vec<u8>> {
    let subdir = dir.join("apple2");

    for name in APPLE2_ROM_NAMES {
        if let Some(data) = try_load(&subdir.join(name)) {
            return Some(data);
        }
        if let Some(data) = try_load(&dir.join(name)) {
            return Some(data);
        }
    }
    None
}

/// Disk II boot ROM file names (P5 PROM, 256 bytes).
const DISK_II_ROM_NAMES: &[&str] = &[
    "diskII.c600.c6ff.bin", "disk2.rom", "diskii.rom",
    "DISK2.ROM", "DiskII.rom",
];

/// Load Disk II boot ROM from the given directory.
pub fn load_disk_ii_rom(dir: &Path) -> Option<Vec<u8>> {
    let subdir = dir.join("apple2");

    for name in DISK_II_ROM_NAMES {
        if let Some(data) = try_load(&subdir.join(name)) {
            return Some(data);
        }
        if let Some(data) = try_load(&dir.join(name)) {
            return Some(data);
        }
    }
    None
}

/// Resolve the system ROMs directory path.
/// If the configured path is relative, resolves it relative to the current directory.
pub fn resolve_roms_dir(configured: &str) -> PathBuf {
    let path = PathBuf::from(configured);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}
