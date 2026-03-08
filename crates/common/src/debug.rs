/// Snapshot of CPU register state for the debugger.
#[derive(Debug, Clone, Default)]
pub struct CpuDebugState {
    pub pc:     u16,
    pub sp:     u8,
    pub a:      u8,
    pub x:      u8,
    pub y:      u8,
    /// Status register (NV-BDIZC)
    pub flags:  u8,
    pub cycles: u64,
}

/// A named group of key→value rows shown in the system-specific debugger tab.
#[derive(Debug, Clone)]
pub struct DebugSection {
    pub name: String,
    pub rows: Vec<(String, String)>,
}

impl DebugSection {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), rows: Vec::new() }
    }
    pub fn row(mut self, key: impl Into<String>, val: impl Into<String>) -> Self {
        self.rows.push((key.into(), val.into()));
        self
    }
}
