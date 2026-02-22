use emu_common::FrameBuffer;

/// VIC-II display dimensions (PAL).
pub const DISPLAY_WIDTH: u32 = 320;
pub const DISPLAY_HEIGHT: u32 = 200;
/// With borders
pub const SCREEN_WIDTH: u32 = 403;
pub const SCREEN_HEIGHT: u32 = 284;

/// VIC-II video chip emulation.
pub struct VicII {
    pub registers: [u8; 64],
    pub framebuffer: FrameBuffer,

    // Internal state
    raster_line: u16,
    raster_irq_line: u16,
    cycle: u16,

    pub irq_pending: bool,

    // Color RAM (1KB at $D800-$DBFF)
    pub color_ram: [u8; 1024],

    frame_ready: bool,
}

/// C64 color palette (16 colors).
static C64_PALETTE: [u32; 16] = [
    0x000000, // 0: black
    0xFFFFFF, // 1: white
    0x880000, // 2: red
    0xAAFFEE, // 3: cyan
    0xCC44CC, // 4: purple
    0x00CC55, // 5: green
    0x0000AA, // 6: blue
    0xEEEE77, // 7: yellow
    0xDD8855, // 8: orange
    0x664400, // 9: brown
    0xFF7777, // 10: light red
    0x333333, // 11: dark grey
    0x777777, // 12: grey
    0xAAFF66, // 13: light green
    0x0088FF, // 14: light blue
    0xBBBBBB, // 15: light grey
];

impl VicII {
    pub fn new() -> Self {
        Self {
            registers: [0; 64],
            framebuffer: FrameBuffer::new(SCREEN_WIDTH, SCREEN_HEIGHT),
            raster_line: 0,
            raster_irq_line: 0,
            cycle: 0,
            irq_pending: false,
            color_ram: [0; 1024],
            frame_ready: false,
        }
    }

    pub fn is_frame_ready(&mut self) -> bool {
        let r = self.frame_ready;
        self.frame_ready = false;
        r
    }

    pub fn read_register(&mut self, addr: u16) -> u8 {
        let reg = (addr & 0x3F) as usize;
        match reg {
            0x11 => {
                // Control register 1 + raster bit 8
                (self.registers[0x11] & 0x7F) | ((self.raster_line as u8 & 0x01) << 7)
            }
            0x12 => {
                // Raster counter low 8 bits
                (self.raster_line & 0xFF) as u8
            }
            0x19 => {
                // Interrupt register
                self.registers[0x19]
            }
            _ => self.registers[reg.min(63)],
        }
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
        let reg = (addr & 0x3F) as usize;
        match reg {
            0x11 => {
                self.registers[0x11] = val;
                self.raster_irq_line = (self.raster_irq_line & 0xFF)
                    | (((val as u16) & 0x80) << 1);
            }
            0x12 => {
                self.raster_irq_line = (self.raster_irq_line & 0x100) | val as u16;
            }
            0x19 => {
                // Acknowledge interrupts (write 1 to clear)
                self.registers[0x19] &= !val;
                if self.registers[0x19] & self.registers[0x1A] == 0 {
                    self.irq_pending = false;
                }
            }
            0x1A => {
                self.registers[0x1A] = val;
            }
            _ => {
                if reg < 64 {
                    self.registers[reg] = val;
                }
            }
        }
    }

    /// Step the VIC-II by one CPU cycle (8 pixels NTSC / PAL).
    pub fn step(&mut self, ram: &[u8; 65536], char_rom: &[u8]) {
        self.cycle += 1;

        if self.cycle >= 63 {
            // End of scanline
            self.render_scanline(ram, char_rom);
            self.cycle = 0;
            self.raster_line += 1;

            // Check raster IRQ
            if self.raster_line == self.raster_irq_line {
                self.registers[0x19] |= 0x01; // Raster IRQ flag
                if self.registers[0x1A] & 0x01 != 0 {
                    self.registers[0x19] |= 0x80; // Set IRQ flag
                    self.irq_pending = true;
                }
            }

            if self.raster_line >= 312 {
                // End of frame (PAL: 312 lines)
                self.raster_line = 0;
                self.frame_ready = true;
            }
        }
    }

    fn render_scanline(&mut self, ram: &[u8; 65536], char_rom: &[u8]) {
        let y = self.raster_line;
        if y >= SCREEN_HEIGHT as u16 {
            return;
        }

        let border_color = C64_PALETTE[(self.registers[0x20] & 0x0F) as usize];
        let bg_color = C64_PALETTE[(self.registers[0x21] & 0x0F) as usize];

        // Check if we're in the visible display area
        let display_y = y as i32 - 51; // First visible line offset
        let in_display = display_y >= 0 && display_y < DISPLAY_HEIGHT as i32;

        for x in 0..SCREEN_WIDTH {
            let display_x = x as i32 - 42; // Horizontal display offset
            let in_display_x = display_x >= 0 && display_x < DISPLAY_WIDTH as i32;

            if in_display && in_display_x {
                // Render character mode
                let dx = display_x as u32;
                let dy = display_y as u32;

                let char_col = dx / 8;
                let char_row = dy / 8;
                let pixel_y = dy % 8;
                let pixel_x = dx % 8;

                // Screen memory base (from registers $D018)
                let screen_base = ((self.registers[0x18] as u16 >> 4) & 0x0F) * 0x400;
                let char_base = ((self.registers[0x18] as u16 >> 1) & 0x07) * 0x800;

                let screen_addr = screen_base + char_row as u16 * 40 + char_col as u16;
                let char_code = ram[screen_addr as usize];

                // Get character bitmap
                let bitmap = if char_base == 0x1000 || char_base == 0x1800 {
                    // Character ROM
                    let idx = char_code as usize * 8 + pixel_y as usize;
                    char_rom.get(idx).copied().unwrap_or(0)
                } else {
                    let addr = char_base + char_code as u16 * 8 + pixel_y as u16;
                    ram[addr as usize]
                };

                let bit = (bitmap >> (7 - pixel_x)) & 1;
                let color = if bit != 0 {
                    let color_addr = char_row * 40 + char_col;
                    let fg = self.color_ram[color_addr as usize] & 0x0F;
                    C64_PALETTE[fg as usize]
                } else {
                    bg_color
                };

                self.framebuffer.set_pixel_rgb(x, y as u32, color);
            } else {
                self.framebuffer.set_pixel_rgb(x, y as u32, border_color);
            }
        }
    }

    /// Get VIC-II bank from CIA2 settings.
    pub fn vic_bank(&self) -> u16 {
        // This would normally come from CIA2 port A
        0 // Default: bank 0 ($0000-$3FFF)
    }
}
