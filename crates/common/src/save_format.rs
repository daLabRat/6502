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
