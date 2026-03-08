use emu_common::FrameBuffer;
use crate::snapshot::VicSnapshot;

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

    // CIA2-derived VIC bank (set by bus before each step)
    pub(crate) vic_bank_base: u16,

    // Sprite-sprite collision register ($D01E) — latched, cleared on read
    sprite_sprite_collision: u8,
    // Sprite-background collision register ($D01F) — latched, cleared on read
    sprite_bg_collision: u8,

    // Badline stall cycles to steal from CPU
    pub(crate) stall_cycles: u8,
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
            vic_bank_base: 0,
            sprite_sprite_collision: 0,
            sprite_bg_collision: 0,
            stall_cycles: 0,
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
                (self.raster_line & 0xFF) as u8
            }
            0x1E => {
                // Sprite-sprite collision — cleared on read
                let val = self.sprite_sprite_collision;
                self.sprite_sprite_collision = 0;
                val
            }
            0x1F => {
                // Sprite-background collision — cleared on read
                let val = self.sprite_bg_collision;
                self.sprite_bg_collision = 0;
                val
            }
            0x19 => {
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
            0x1E | 0x1F => {
                // Collision registers are read-only
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

            // Badline detection: display area AND (raster & 7) == YSCROLL
            let display_on = self.registers[0x11] & 0x10 != 0;
            let yscroll = (self.registers[0x11] & 0x07) as u16;
            let in_display_area = self.raster_line >= 0x30 && self.raster_line < 0xF8;
            if display_on && in_display_area && (self.raster_line & 7) == yscroll {
                self.stall_cycles = 40;
            }

            // Check raster IRQ
            if self.raster_line == self.raster_irq_line {
                self.registers[0x19] |= 0x01; // Raster IRQ flag
                if self.registers[0x1A] & 0x01 != 0 {
                    self.registers[0x19] |= 0x80;
                    self.irq_pending = true;
                }
            }

            if self.raster_line >= 312 {
                self.raster_line = 0;
                self.frame_ready = true;
            }
        }
    }

    /// Read a byte from VIC-II's address space (respecting bank and char ROM).
    #[inline]
    fn vic_read(&self, addr: u16, ram: &[u8; 65536], char_rom: &[u8]) -> u8 {
        let full_addr = self.vic_bank_base.wrapping_add(addr);
        // Character ROM is visible at $1000-$1FFF relative to bank base,
        // but only in banks 0 ($0000) and 2 ($8000).
        let bank = self.vic_bank_base;
        if (bank == 0x0000 || bank == 0x8000) && addr >= 0x1000 && addr < 0x2000 {
            let idx = (addr - 0x1000) as usize;
            char_rom.get(idx).copied().unwrap_or(0)
        } else {
            ram[full_addr as usize]
        }
    }

    fn render_scanline(&mut self, ram: &[u8; 65536], char_rom: &[u8]) {
        let y = self.raster_line;
        if y >= SCREEN_HEIGHT as u16 {
            return;
        }

        let border_color = C64_PALETTE[(self.registers[0x20] & 0x0F) as usize];
        let bg_color0 = C64_PALETTE[(self.registers[0x21] & 0x0F) as usize];

        let ctrl1 = self.registers[0x11];
        let ctrl2 = self.registers[0x16];
        let display_on = ctrl1 & 0x10 != 0;
        let bmm = ctrl1 & 0x20 != 0;   // Bitmap mode
        let ecm = ctrl1 & 0x40 != 0;   // Extended color mode
        let mcm = ctrl2 & 0x10 != 0;   // Multicolor mode
        let xscroll = (ctrl2 & 0x07) as i32;
        let yscroll = (ctrl1 & 0x07) as i32;
        let col38 = ctrl2 & 0x08 == 0; // 38-column mode
        let row24 = ctrl1 & 0x08 == 0; // 24-row mode

        let display_y_start = 51;
        let display_y = y as i32 - display_y_start;
        let in_display = display_y >= 0 && display_y < DISPLAY_HEIGHT as i32 && display_on;

        // Screen and char/bitmap base addresses (within VIC bank)
        let screen_base = ((self.registers[0x18] as u16 >> 4) & 0x0F) * 0x400;
        let char_base = ((self.registers[0x18] as u16 >> 1) & 0x07) * 0x800;
        let bitmap_base = if self.registers[0x18] & 0x08 != 0 { 0x2000u16 } else { 0x0000u16 };

        // Background scanline buffer for sprite-bg collision detection
        let mut bg_opaque = [false; 320];

        for x in 0..SCREEN_WIDTH {
            let display_x = x as i32 - 42;
            let in_display_x = display_x >= 0 && display_x < DISPLAY_WIDTH as i32;

            // Border narrowing
            let in_border = if in_display && in_display_x {
                let dx = display_x as u32;
                let dy = display_y as u32;
                (col38 && (dx < 8 || dx >= 312))
                || (row24 && (dy < 8 || dy >= 192))
            } else {
                true
            };

            if in_display && in_display_x && !in_border {
                let dx = display_x as u32;
                let dy = display_y as u32;

                // Apply scroll offset
                let scrolled_x = dx.wrapping_sub(xscroll as u32) & 0x1FF;
                let scrolled_y = dy.wrapping_sub(yscroll as u32) & 0xFF;

                let char_col = scrolled_x / 8;
                let char_row = scrolled_y / 8;
                let pixel_y = scrolled_y % 8;
                let pixel_x = scrolled_x % 8;

                if char_col >= 40 || char_row >= 25 {
                    self.framebuffer.set_pixel_rgb(x, y as u32, bg_color0);
                    continue;
                }

                let screen_addr = screen_base + char_row as u16 * 40 + char_col as u16;
                let screen_byte = self.vic_read(screen_addr, ram, char_rom);
                let color_ram_byte = self.color_ram[(char_row * 40 + char_col) as usize] & 0x0F;

                let (color, is_fg) = if bmm && mcm {
                    // Multicolor bitmap mode (160×200, 2 bits per pixel)
                    let bitmap_addr = bitmap_base + char_row as u16 * 320 + char_col as u16 * 8 + pixel_y as u16;
                    let bitmap_byte = self.vic_read(bitmap_addr, ram, char_rom);
                    let mc_px = (pixel_x / 2) as u8; // 0-3 multicolor pixel
                    let shift = (3 - mc_px) * 2;
                    let bits = (bitmap_byte >> shift) & 0x03;
                    match bits {
                        0 => (C64_PALETTE[(self.registers[0x21] & 0x0F) as usize], false),
                        1 => (C64_PALETTE[(screen_byte >> 4) as usize], true),
                        2 => (C64_PALETTE[(screen_byte & 0x0F) as usize], true),
                        _ => (C64_PALETTE[color_ram_byte as usize], true),
                    }
                } else if bmm {
                    // Standard bitmap mode (320×200, 1 bit per pixel)
                    let bitmap_addr = bitmap_base + char_row as u16 * 320 + char_col as u16 * 8 + pixel_y as u16;
                    let bitmap_byte = self.vic_read(bitmap_addr, ram, char_rom);
                    let bit = (bitmap_byte >> (7 - pixel_x)) & 1;
                    if bit != 0 {
                        (C64_PALETTE[(screen_byte >> 4) as usize], true)
                    } else {
                        (C64_PALETTE[(screen_byte & 0x0F) as usize], false)
                    }
                } else if ecm {
                    // Extended color mode: upper 2 bits of char code select bg color
                    let bg_select = (screen_byte >> 6) & 0x03;
                    let char_code = screen_byte & 0x3F;
                    let char_addr = char_base + char_code as u16 * 8 + pixel_y as u16;
                    let bitmap = self.vic_read(char_addr, ram, char_rom);
                    let bit = (bitmap >> (7 - pixel_x)) & 1;
                    if bit != 0 {
                        (C64_PALETTE[color_ram_byte as usize], true)
                    } else {
                        let bg_reg = 0x21 + bg_select as usize;
                        (C64_PALETTE[(self.registers[bg_reg] & 0x0F) as usize], false)
                    }
                } else if mcm {
                    // Multicolor character mode
                    let char_addr = char_base + screen_byte as u16 * 8 + pixel_y as u16;
                    let bitmap = self.vic_read(char_addr, ram, char_rom);

                    if color_ram_byte & 0x08 != 0 {
                        // This character uses multicolor (color RAM bit 3 set)
                        let mc_px = (pixel_x / 2) as u8;
                        let shift = (3 - mc_px) * 2;
                        let bits = (bitmap >> shift) & 0x03;
                        match bits {
                            0 => (C64_PALETTE[(self.registers[0x21] & 0x0F) as usize], false),
                            1 => (C64_PALETTE[(self.registers[0x22] & 0x0F) as usize], true),
                            2 => (C64_PALETTE[(self.registers[0x23] & 0x0F) as usize], true),
                            _ => (C64_PALETTE[(color_ram_byte & 0x07) as usize], true),
                        }
                    } else {
                        // Standard hires character (color RAM bit 3 clear)
                        let bit = (bitmap >> (7 - pixel_x)) & 1;
                        if bit != 0 {
                            (C64_PALETTE[color_ram_byte as usize], true)
                        } else {
                            (bg_color0, false)
                        }
                    }
                } else {
                    // Standard character mode
                    let char_addr = char_base + screen_byte as u16 * 8 + pixel_y as u16;
                    let bitmap = self.vic_read(char_addr, ram, char_rom);
                    let bit = (bitmap >> (7 - pixel_x)) & 1;
                    if bit != 0 {
                        (C64_PALETTE[color_ram_byte as usize], true)
                    } else {
                        (bg_color0, false)
                    }
                };

                if is_fg && dx < 320 {
                    bg_opaque[dx as usize] = true;
                }

                self.framebuffer.set_pixel_rgb(x, y as u32, color);
            } else {
                self.framebuffer.set_pixel_rgb(x, y as u32, border_color);
            }
        }

        // Sprite rendering pass
        self.render_sprites(y, ram, char_rom, &bg_opaque);
    }

    /// Render sprites for the given scanline, compositing on top of the framebuffer.
    fn render_sprites(&mut self, y: u16, ram: &[u8; 65536], char_rom: &[u8], bg_opaque: &[bool; 320]) {
        let sprite_enable = self.registers[0x15];
        if sprite_enable == 0 {
            return;
        }

        let display_y_start = 51i16;
        let mcm_sprites = self.registers[0x1C];
        let x_expand = self.registers[0x1D];
        let y_expand = self.registers[0x17];
        let priority = self.registers[0x1B]; // 0 = sprite in front, 1 = behind background
        let x_hi = self.registers[0x10]; // X position bit 8

        let screen_base = ((self.registers[0x18] as u16 >> 4) & 0x0F) * 0x400;

        let mc_color0 = self.registers[0x25] & 0x0F;
        let mc_color1 = self.registers[0x26] & 0x0F;

        // Track which sprites have pixels on which x positions (for collision)
        let mut sprite_pixels: [u16; 403] = [0; 403]; // bitmask of which sprites have pixels at each x

        // Render from highest to lowest priority (sprite 7 first, 0 last = on top)
        for spr in (0..8u8).rev() {
            if sprite_enable & (1 << spr) == 0 {
                continue;
            }

            let spr_y = self.registers[spr as usize * 2 + 1] as i16;
            let spr_x = self.registers[spr as usize * 2] as u16
                | if x_hi & (1 << spr) != 0 { 0x100 } else { 0 };

            let y_exp = y_expand & (1 << spr) != 0;
            let spr_height = if y_exp { 42 } else { 21 };

            // Check if this sprite is on this scanline
            // Sprite Y is in screen coordinates; raster Y is also screen coordinates
            let row = y as i16 - spr_y - display_y_start + 1;
            if row < 0 || row >= spr_height {
                continue;
            }

            let data_row = if y_exp { row / 2 } else { row } as u16;

            // Sprite data pointer at screen_base + $3F8 + sprite_num
            let pointer_addr = screen_base + 0x03F8 + spr as u16;
            let sprite_block = self.vic_read(pointer_addr, ram, char_rom) as u16;
            let data_addr = sprite_block * 64 + data_row * 3;

            let b0 = self.vic_read(data_addr, ram, char_rom);
            let b1 = self.vic_read(data_addr + 1, ram, char_rom);
            let b2 = self.vic_read(data_addr + 2, ram, char_rom);

            let x_exp = x_expand & (1 << spr) != 0;
            let is_mc = mcm_sprites & (1 << spr) != 0;
            let behind_bg = priority & (1 << spr) != 0;
            let spr_color = self.registers[0x27 + spr as usize] & 0x0F;

            let pixel_width = if x_exp { 2 } else { 1 };

            // Decode 24 bits of sprite data
            let data: u32 = (b0 as u32) << 16 | (b1 as u32) << 8 | b2 as u32;

            if is_mc {
                // Multicolor sprite: 12 double-width pixels, 2 bits each
                for px in 0..12u16 {
                    let bit_pos = 22 - px * 2;
                    let bits = ((data >> bit_pos) & 0x03) as u8;
                    if bits == 0 { continue; }

                    let color = match bits {
                        1 => mc_color0,
                        2 => spr_color,
                        3 => mc_color1,
                        _ => continue,
                    };

                    for sub in 0..(pixel_width * 2) {
                        let screen_x = spr_x as i32 + px as i32 * pixel_width * 2 + sub as i32 - 24;
                        self.plot_sprite_pixel(
                            screen_x, y, spr, color, behind_bg,
                            bg_opaque, &mut sprite_pixels,
                        );
                    }
                }
            } else {
                // Hires sprite: 24 single-width pixels
                for px in 0..24u16 {
                    let bit = (data >> (23 - px)) & 1;
                    if bit == 0 { continue; }

                    for sub in 0..pixel_width {
                        let screen_x = spr_x as i32 + px as i32 * pixel_width + sub as i32 - 24;
                        self.plot_sprite_pixel(
                            screen_x, y, spr, spr_color, behind_bg,
                            bg_opaque, &mut sprite_pixels,
                        );
                    }
                }
            }
        }

        // Update collision registers from sprite_pixels
        for x in 0..SCREEN_WIDTH as usize {
            let mask = sprite_pixels[x];
            if mask == 0 { continue; }

            // Sprite-sprite: if more than one sprite has a pixel here
            if mask & (mask - 1) != 0 {
                // Multiple sprites present
                self.sprite_sprite_collision |= mask as u8;
            }
        }
    }

    pub fn snapshot(&self) -> VicSnapshot {
        VicSnapshot {
            registers: self.registers.to_vec(),
            raster_line: self.raster_line,
            raster_irq_line: self.raster_irq_line,
            cycle: self.cycle,
            irq_pending: self.irq_pending,
            color_ram: self.color_ram.to_vec(),
            vic_bank_base: self.vic_bank_base,
            sprite_sprite_collision: self.sprite_sprite_collision,
            sprite_bg_collision: self.sprite_bg_collision,
            stall_cycles: self.stall_cycles,
        }
    }

    pub fn restore(&mut self, s: &VicSnapshot) {
        if s.registers.len() != 64 {
            log::warn!("VIC restore: unexpected registers length {}", s.registers.len());
            return;
        }
        if s.color_ram.len() != 1024 {
            log::warn!("VIC restore: unexpected color_ram length {}", s.color_ram.len());
            return;
        }
        self.registers.copy_from_slice(&s.registers);
        self.raster_line = s.raster_line;
        self.raster_irq_line = s.raster_irq_line;
        self.cycle = s.cycle;
        self.irq_pending = s.irq_pending;
        self.color_ram.copy_from_slice(&s.color_ram);
        self.vic_bank_base = s.vic_bank_base;
        self.sprite_sprite_collision = s.sprite_sprite_collision;
        self.sprite_bg_collision = s.sprite_bg_collision;
        self.stall_cycles = s.stall_cycles;
        self.frame_ready = false;
    }

    /// Plot a single sprite pixel, handling priority and collision.
    #[inline]
    fn plot_sprite_pixel(
        &mut self,
        screen_x: i32,
        y: u16,
        spr: u8,
        color: u8,
        behind_bg: bool,
        bg_opaque: &[bool; 320],
        sprite_pixels: &mut [u16; 403],
    ) {
        // screen_x is in display coordinates (0-based)
        let fb_x = (screen_x + 42) as u32;
        if fb_x >= SCREEN_WIDTH || y >= SCREEN_HEIGHT as u16 {
            return;
        }

        let dx = screen_x as usize;

        // Track for collision
        if dx < 320 {
            sprite_pixels[fb_x as usize] |= 1 << spr;

            // Sprite-background collision
            if bg_opaque[dx] {
                self.sprite_bg_collision |= 1 << spr;
            }
        }

        // Render: behind-bg sprites only show where background is transparent
        if behind_bg && dx < 320 && bg_opaque[dx] {
            return;
        }

        self.framebuffer.set_pixel_rgb(fb_x, y as u32, C64_PALETTE[color as usize]);
    }
}
