/// 1541 drive internal bus (implements `emu_common::Bus`).
///
/// Address map:
///   $0000-$07FF: 2KB RAM
///   $1800-$180F: VIA1 (IEC serial bus interface)
///   $1C00-$1C0F: VIA2 (drive mechanics: stepper, motor, read/write)
///   $C000-$FFFF: 16KB ROM

use emu_common::Bus;
use crate::via::Via;
use crate::iec_bus::IecBus;
use super::GcrDisk;

/// The 1541 drive's internal bus.
pub struct Drive1541Bus {
    pub(crate) ram: [u8; 2048],
    pub(crate) rom: Vec<u8>,   // 16KB ROM
    pub(crate) via1: Via,       // IEC bus interface
    pub(crate) via2: Via,       // Drive mechanics
    pub(crate) disk: GcrDisk,

    // Last stepper phase for edge detection
    last_stepper_phase: u8,
    // Last motor state for change detection
    last_motor_on: bool,
    // Bytes logged since motor-on (capped to avoid log flood)
    disk_log_count: u32,

    /// SO (Set Overflow) pin pending: true when a BYTE READY pulse occurred this tick.
    /// The 1541 firmware polls BVS (CLV + BVC loop) to wait for bytes from disk.
    so_pending: bool,

    /// Count of consecutive 0xFF bytes received from disk.
    /// The 1541 sync detector fires on 10+ consecutive serial 1-bits; in our
    /// byte-at-a-time model that requires ≥2 consecutive 0xFF bytes (16+ bits).
    /// An isolated 0xFF in GCR data (valid: e.g. g3=0x0F, g4=0x1E share a byte
    /// boundary) must NOT suppress BYTE READY — only a true sync run should.
    consecutive_ff: u8,
}

impl Drive1541Bus {
    pub fn new(rom: Vec<u8>) -> Self {
        let mut bus = Self {
            ram: [0; 2048],
            rom,
            via1: Via::new(),
            via2: Via::new(),
            disk: GcrDisk::new(),
            last_stepper_phase: 0,
            last_motor_on: false,
            disk_log_count: 0,
            so_pending: false,
            consecutive_ff: 0,
        };
        // Device 8 address select: PB5=0, PB6=0 (hardware jumpers grounded)
        // The firmware reads these pins at $EB3C to determine the IEC device number.
        bus.via1.irb &= !0x60;
        bus
    }

    /// Synchronize IEC bus state: update VIA1 inputs from the shared IEC bus.
    pub fn sync_iec_input(&mut self, iec: &IecBus) {
        // VIA1 Port A: ATN on bit 7 (inverted through hardware)
        self.via1.ira = (self.via1.ira & 0x7F) | iec.drive_via1_port_a_input();
        // VIA1 Port B: DATA in bit 0, CLK in bit 2, ATN in bit 7 (all inverted)
        let bus_bits = iec.drive_via1_port_b_input();
        let atn_pb7 = if iec.atn() { 0x00 } else { 0x80 }; // inverted: bus low → bit high
        self.via1.irb = (self.via1.irb & !0x85) | bus_bits | atn_pb7;

        // ATN edge detection: CA1 on VIA1 is connected to ATN (inverted)
        self.via1.ca1_input = !iec.atn(); // CA1 high when ATN asserted (bus low)
    }

    /// Push IEC bus state: update the shared IEC bus from VIA1 outputs.
    /// Includes the ATN auto-acknowledge XOR circuit present on real 1541 hardware.
    pub fn sync_iec_output(&self, iec: &mut IecBus) {
        // Only bits configured as output (DDRB=1) drive the bus
        let data_out = self.via1.orb & self.via1.ddrb & 0x02 != 0;
        let clk_out = self.via1.orb & self.via1.ddrb & 0x08 != 0;

        // ATN auto-acknowledge circuit (hardware XOR gate on the 1541 board):
        // When ATN is asserted (bus low → inverted to HIGH inside 1541),
        // XOR with PB4 (ATN ACK) output. If result is HIGH, pull DATA low.
        // This gives immediate DATA acknowledgment when ATN is asserted,
        // before the drive firmware even runs its ISR.
        let atn_inverted = !iec.atn(); // true when ATN line is low (asserted)
        let pb4_out = self.via1.orb & self.via1.ddrb & 0x10 != 0;
        let auto_data = atn_inverted ^ pb4_out;

        // Drive pulls DATA if PB1 output set OR auto-acknowledge active
        iec.drive_data = data_out || auto_data;
        iec.drive_clk = clk_out;
    }

    /// Step the drive mechanics (called each drive CPU cycle).
    fn step_mechanics(&mut self) {
        // Read VIA2 Port B for motor and stepper control
        let pb = self.via2.port_b_output();

        // Motor control: bit 2
        let motor_on = pb & 0x04 != 0;
        if motor_on != self.last_motor_on {
            log::info!("[DISK] Motor {} trk={}", if motor_on { "ON" } else { "OFF" }, self.disk.half_track / 2 + 1);
            self.last_motor_on = motor_on;
            self.disk_log_count = 0;
            self.consecutive_ff = 0; // Reset sync state on motor change
        }
        self.disk.motor_on = motor_on;

        // Stepper motor: bits 0-1
        let stepper_phase = pb & 0x03;
        if stepper_phase != self.last_stepper_phase {
            let old_track = self.disk.half_track / 2 + 1;
            self.disk.step_head(stepper_phase);
            self.last_stepper_phase = stepper_phase;
            let new_track = self.disk.half_track / 2 + 1;
            if new_track != old_track {
                log::info!("[DISK] Seek trk={}", new_track);
                self.disk_log_count = 0; // Reset byte log cap on track change
                self.consecutive_ff = 0; // Reset sync state after seek
            }
        }

        // Step disk rotation
        self.disk.step();

        // When a byte is ready from the disk, deliver it to VIA2 Port A,
        // pulse VIA2 CA1, and assert SO pin (BYTE READY).
        //
        // On real 1541 hardware, the sync detection circuit suppresses BYTE READY
        // (both the SO pin and CA1) while reading sync bytes (consecutive $FF /
        // 10+ consecutive 1-bits). SO and CA1 only fire for the first actual data
        // byte after sync. This is critical for the IRQ-driven disk read ISR
        // ($F2B0): if CA1 fired during sync bytes, the ISR would process $FF bytes
        // as data bytes, causing GCR data checksum errors (status $05).
        if self.disk.byte_ready {
            self.disk.byte_ready = false;
            self.via2.ira = self.disk.current_byte;

            // VIA2 Port B bit 7 = ~SYNC (hardware sync detect):
            // LOW (0) when 10+ consecutive 1-bits are read from the serial GCR stream.
            // In our byte-at-a-time model: a single 0xFF = 8 serial 1s (not enough);
            // only 2+ consecutive 0xFF bytes = 16+ serial 1s triggers SYNC.
            //
            // This matters because valid GCR-encoded data can produce isolated 0xFF
            // bytes (e.g. when GCR nibble codes 0x0F and 0x1E share a byte boundary).
            // Treating isolated 0xFF as sync suppresses BYTE READY, causing the BVC
            // read loop to skip the byte and corrupt GCR decoding (status $05 error).
            if self.disk.current_byte == 0xFF {
                self.consecutive_ff = self.consecutive_ff.saturating_add(1);
            } else {
                self.consecutive_ff = 0;
            }
            let is_sync = self.consecutive_ff >= 2;

            if is_sync {
                self.via2.irb &= !0x80; // bit 7 = 0: SYNC detected
                self.via2.ca1_input = false; // Suppress CA1 and SO during sync
            } else {
                self.via2.irb |= 0x80;  // bit 7 = 1: no sync
                self.via2.ca1_input = true;  // Pulse CA1 (triggers byte-ready IRQ)
                self.so_pending = true;  // Assert SO (Set Overflow) = BYTE READY
            }
            if self.disk_log_count < 300 {
                log::debug!("[DISK] trk={} pos={} byte={:02X}",
                    self.disk.half_track / 2 + 1,
                    self.disk.byte_position,
                    self.disk.current_byte);
                self.disk_log_count += 1;
            }
        } else {
            self.via2.ca1_input = false;
        }

        // Write protect sense: VIA2 Port B bit 4 (active low = protected)
        if self.disk.write_protect {
            self.via2.irb &= !0x10;
        } else {
            self.via2.irb |= 0x10;
        }
    }
}

impl Bus for Drive1541Bus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0FFF => self.ram[(addr & 0x07FF) as usize],
            0x1800..=0x1BFF => self.via1.read(addr), // VIA1 mirrors every 16 bytes
            0x1C00..=0x1FFF => self.via2.read(addr), // VIA2 mirrors every 16 bytes
            0xC000..=0xFFFF => {
                let idx = (addr - 0xC000) as usize;
                self.rom.get(idx).copied().unwrap_or(0xFF)
            }
            _ => 0xFF, // Open bus
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x0FFF => {
                self.ram[(addr & 0x07FF) as usize] = val;
            }
            0x1800..=0x1BFF => self.via1.write(addr, val),
            0x1C00..=0x1FFF => self.via2.write(addr, val),
            0xC000..=0xFFFF => {} // ROM — ignore writes
            _ => {}
        }
    }

    fn peek(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x0FFF => self.ram[(addr & 0x07FF) as usize],
            0xC000..=0xFFFF => {
                let idx = (addr - 0xC000) as usize;
                self.rom.get(idx).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    fn tick(&mut self, cycles: u8) {
        for _ in 0..cycles {
            self.via1.step();
            self.via2.step();
            self.step_mechanics();
        }
    }

    fn poll_nmi(&mut self) -> bool {
        false // 1541 has no NMI
    }

    fn poll_irq(&mut self) -> bool {
        self.via1.irq_pending() || self.via2.irq_pending()
    }

    fn poll_so(&mut self) -> bool {
        // BYTE READY asserts SO pin (set overflow) for the 1541's BVC polling loops
        let pending = self.so_pending;
        self.so_pending = false;
        pending
    }
}
