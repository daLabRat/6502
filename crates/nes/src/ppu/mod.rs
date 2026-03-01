pub mod palette;
pub mod renderer;

use emu_common::FrameBuffer;

/// NES PPU (2C02) - Picture Processing Unit.
/// Renders 256x240 pixels, scanline-by-scanline.
pub struct Ppu {
    // VRAM
    pub(crate) nametable_ram: [u8; 2048], // 2KB nametable RAM
    pub(crate) palette_ram: [u8; 32],      // palette RAM
    pub(crate) oam: [u8; 256],             // Object Attribute Memory (64 sprites)

    // Registers
    pub(crate) ctrl: u8,    // $2000 PPUCTRL
    pub(crate) mask: u8,    // $2001 PPUMASK
    pub(crate) status: u8,  // $2002 PPUSTATUS
    pub(crate) oam_addr: u8, // $2003 OAMADDR

    // Internal state
    pub(crate) v: u16,      // Current VRAM address (15 bit)
    pub(crate) t: u16,      // Temporary VRAM address (15 bit)
    pub(crate) fine_x: u8,  // Fine X scroll (3 bit)
    pub(crate) w: bool,     // Write toggle (first/second write)

    pub(crate) data_buffer: u8, // PPUDATA read buffer (delayed read)

    // Timing
    pub(crate) scanline: i16,  // -1 to 260
    pub(crate) cycle: u16,     // 0 to 340
    pub(crate) frame_count: u64,

    // Flags
    pub(crate) nmi_pending: bool,
    pub(crate) frame_ready: bool,

    // Output
    pub(crate) framebuffer: FrameBuffer,

    // Sprite evaluation scratch
    pub(crate) sprite_scanline: [(u8, u8, u8, u8); 8], // up to 8 sprites per scanline
    pub(crate) sprite_count: u8,
    pub(crate) sprite_zero_on_line: bool, // is sprite 0 in sprite_scanline?

    // Scanline buffers for rendering
    pub(crate) bg_pixel_buffer: [u8; 272],    // 34 tiles * 8 pixels (extra for fine_x)
    pub(crate) bg_palette_buffer: [u8; 272],  // palette index per pixel
    pub(crate) sprite_pixel_buffer: [u8; 256],
    pub(crate) sprite_palette_buffer: [u8; 256],
    pub(crate) sprite_priority_buffer: [bool; 256],
    pub(crate) sprite_zero_buffer: [bool; 256],
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            nametable_ram: [0; 2048],
            palette_ram: [0; 32],
            oam: [0; 256],
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            data_buffer: 0,
            scanline: -1,
            cycle: 0,
            frame_count: 0,
            nmi_pending: false,
            frame_ready: false,
            framebuffer: FrameBuffer::new(256, 240),
            sprite_scanline: [(0, 0, 0, 0); 8],
            sprite_count: 0,
            sprite_zero_on_line: false,
            bg_pixel_buffer: [0; 272],
            bg_palette_buffer: [0; 272],
            sprite_pixel_buffer: [0; 256],
            sprite_palette_buffer: [0; 256],
            sprite_priority_buffer: [false; 256],
            sprite_zero_buffer: [false; 256],
        }
    }

    /// Read a PPU register (CPU-mapped at $2000-$2007).
    pub fn read_register(&mut self, addr: u16, mapper: &mut dyn crate::cartridge::mapper::Mapper) -> u8 {
        match addr & 0x07 {
            // $2002: PPUSTATUS
            2 => {
                let val = (self.status & 0xE0) | (self.data_buffer & 0x1F);
                self.status &= !0x80; // Clear vblank flag
                self.w = false;       // Reset write toggle
                val
            }
            // $2004: OAMDATA
            4 => self.oam[self.oam_addr as usize],
            // $2007: PPUDATA
            7 => {
                let addr = self.v & 0x3FFF;
                let val = if addr >= 0x3F00 {
                    // Palette reads are not delayed
                    let palette_val = self.read_palette(addr);
                    // But the buffer gets filled with the nametable "under" the palette
                    self.data_buffer = self.ppu_read(addr & 0x2FFF, mapper);
                    palette_val
                } else {
                    let buffered = self.data_buffer;
                    self.data_buffer = self.ppu_read(addr, mapper);
                    buffered
                };
                self.v = self.v.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 });
                val
            }
            _ => 0,
        }
    }

    /// Write a PPU register (CPU-mapped at $2000-$2007).
    pub fn write_register(&mut self, addr: u16, val: u8, mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        match addr & 0x07 {
            // $2000: PPUCTRL
            0 => {
                let was_nmi_enabled = self.ctrl & 0x80 != 0;
                self.ctrl = val;
                // t: ...GH.. ........ <- val: ......GH
                self.t = (self.t & 0xF3FF) | ((val as u16 & 0x03) << 10);
                // If NMI just enabled while in vblank, trigger NMI
                if !was_nmi_enabled && (val & 0x80 != 0) && (self.status & 0x80 != 0) {
                    self.nmi_pending = true;
                }
            }
            // $2001: PPUMASK
            1 => self.mask = val,
            // $2003: OAMADDR
            3 => self.oam_addr = val,
            // $2004: OAMDATA
            4 => {
                self.oam[self.oam_addr as usize] = val;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            // $2005: PPUSCROLL
            5 => {
                if !self.w {
                    // First write: X scroll
                    self.t = (self.t & 0xFFE0) | ((val as u16) >> 3);
                    self.fine_x = val & 0x07;
                } else {
                    // Second write: Y scroll
                    self.t = (self.t & 0x8C1F)
                        | ((val as u16 & 0x07) << 12)
                        | ((val as u16 & 0xF8) << 2);
                }
                self.w = !self.w;
            }
            // $2006: PPUADDR
            6 => {
                if !self.w {
                    // First write: high byte
                    self.t = (self.t & 0x00FF) | ((val as u16 & 0x3F) << 8);
                } else {
                    // Second write: low byte
                    self.t = (self.t & 0xFF00) | val as u16;
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            // $2007: PPUDATA
            7 => {
                let addr = self.v & 0x3FFF;
                self.ppu_write(addr, val, mapper);
                self.v = self.v.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 });
            }
            _ => {}
        }
    }

    /// OAM DMA: write 256 bytes to OAM.
    pub fn write_oam_dma(&mut self, data: &[u8; 256]) {
        for (i, &byte) in data.iter().enumerate() {
            self.oam[(self.oam_addr as usize + i) & 0xFF] = byte;
        }
    }

    /// Step the PPU by one cycle. Called 3 times per CPU cycle.
    pub fn step(&mut self, mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        // Pre-render scanline (-1)
        if self.scanline == -1 {
            if self.cycle == 1 {
                // Clear vblank, sprite overflow, sprite 0 hit
                self.status &= !0xE0;
                self.nmi_pending = false;
            }
            if self.cycle == 257 && self.rendering_enabled() {
                // Copy horizontal bits from t to v
                self.v = (self.v & 0x7BE0) | (self.t & 0x041F);
                // Evaluate sprites for scanline 0
                self.evaluate_sprites(mapper);
                self.fill_sprite_scanline_buffer(mapper);
            }
            if self.cycle >= 280 && self.cycle <= 304 && self.rendering_enabled() {
                // Copy vertical bits from t to v
                self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
            }
            // Scanline tick for mapper (MMC3)
            if self.cycle == 260 && self.rendering_enabled() {
                mapper.scanline_tick();
            }
        }

        // Visible scanlines (0-239)
        if self.scanline >= 0 && self.scanline < 240 {
            // Fill background buffer at cycle 0
            if self.cycle == 0 && self.rendering_enabled() {
                self.fill_bg_scanline_buffer(mapper);
            }

            // Render pixels from buffers (cycles 1-256)
            if self.cycle >= 1 && self.cycle <= 256 {
                self.render_pixel();
            }

            // Coarse X increment every 8 cycles during visible portion
            if self.rendering_enabled() && self.cycle >= 8 && self.cycle <= 248 && self.cycle % 8 == 0 {
                self.increment_x();
            }

            if self.cycle == 256 && self.rendering_enabled() {
                self.increment_y();
            }
            if self.cycle == 257 && self.rendering_enabled() {
                // Copy horizontal bits from t to v
                self.v = (self.v & 0x7BE0) | (self.t & 0x041F);
            }
            // Sprite evaluation at cycle 257, then fill sprite buffer
            if self.cycle == 257 {
                self.evaluate_sprites(mapper);
                if self.rendering_enabled() {
                    self.fill_sprite_scanline_buffer(mapper);
                }
            }
            // Scanline tick for mapper (MMC3)
            if self.cycle == 260 && self.rendering_enabled() {
                mapper.scanline_tick();
            }
        }

        // VBlank start (scanline 241, cycle 1)
        if self.scanline == 241 && self.cycle == 1 {
            self.status |= 0x80; // Set vblank flag
            self.frame_ready = true;
            if self.ctrl & 0x80 != 0 {
                self.nmi_pending = true;
            }
        }

        // Advance timing
        self.cycle += 1;

        // Odd frame skip: on pre-render scanline, if frame_count is odd and
        // rendering is enabled, skip cycle 339 (jump from 338 to 0 of scanline 0).
        if self.scanline == -1 && self.cycle == 339
            && self.frame_count & 1 != 0 && self.rendering_enabled()
        {
            self.cycle = 0;
            self.scanline = 0;
            self.frame_count += 1;
            return;
        }

        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;
            if self.scanline > 260 {
                self.scanline = -1;
                self.frame_count += 1;
            }
        }
    }

    /// Check if rendering is enabled.
    fn rendering_enabled(&self) -> bool {
        self.mask & 0x18 != 0
    }

    /// Internal PPU memory read.
    fn ppu_read(&self, addr: u16, mapper: &mut dyn crate::cartridge::mapper::Mapper) -> u8 {
        match addr {
            0x0000..=0x1FFF => mapper.ppu_read(addr),
            0x2000..=0x3EFF => {
                let mirroring = mapper.mirroring();
                let idx = mirroring.mirror_vram_addr(addr) as usize;
                self.nametable_ram[idx]
            }
            0x3F00..=0x3FFF => self.read_palette(addr),
            _ => 0,
        }
    }

    /// Internal PPU memory write.
    fn ppu_write(&mut self, addr: u16, val: u8, mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        match addr {
            0x0000..=0x1FFF => mapper.ppu_write(addr, val),
            0x2000..=0x3EFF => {
                let mirroring = mapper.mirroring();
                let idx = mirroring.mirror_vram_addr(addr) as usize;
                self.nametable_ram[idx] = val;
            }
            0x3F00..=0x3FFF => self.write_palette(addr, val),
            _ => {}
        }
    }

    /// Read palette RAM with mirroring.
    fn read_palette(&self, addr: u16) -> u8 {
        let idx = Self::mirror_palette_addr(addr) as usize;
        self.palette_ram[idx]
    }

    /// Write palette RAM with mirroring.
    fn write_palette(&mut self, addr: u16, val: u8) {
        let idx = Self::mirror_palette_addr(addr) as usize;
        self.palette_ram[idx] = val;
    }

    /// Mirror palette addresses. $3F10/$3F14/$3F18/$3F1C mirror $3F00/$3F04/$3F08/$3F0C.
    fn mirror_palette_addr(addr: u16) -> u16 {
        let addr = addr & 0x1F;
        match addr {
            0x10 | 0x14 | 0x18 | 0x1C => addr - 0x10,
            _ => addr,
        }
    }

    /// Increment the coarse X position in v.
    fn increment_x(&mut self) {
        if (self.v & 0x001F) == 31 {
            self.v &= !0x001F;         // coarse X = 0
            self.v ^= 0x0400;          // switch horizontal nametable
        } else {
            self.v += 1;
        }
    }

    /// Increment the Y position in v.
    fn increment_y(&mut self) {
        if (self.v & 0x7000) != 0x7000 {
            self.v += 0x1000; // increment fine Y
        } else {
            self.v &= !0x7000; // fine Y = 0
            let mut y = (self.v & 0x03E0) >> 5; // coarse Y
            if y == 29 {
                y = 0;
                self.v ^= 0x0800; // switch vertical nametable
            } else if y == 31 {
                y = 0; // wrap without switching nametable
            } else {
                y += 1;
            }
            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    /// Evaluate sprites for the current scanline.
    fn evaluate_sprites(&mut self, _mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        let sprite_height: i16 = if self.ctrl & 0x20 != 0 { 16 } else { 8 };
        self.sprite_count = 0;
        self.sprite_zero_on_line = false;

        for i in 0..64 {
            let y = self.oam[i * 4] as i16;
            let diff = self.scanline - y;

            if diff >= 0 && diff < sprite_height {
                if self.sprite_count < 8 {
                    if i == 0 {
                        self.sprite_zero_on_line = true;
                    }
                    self.sprite_scanline[self.sprite_count as usize] = (
                        self.oam[i * 4],     // Y
                        self.oam[i * 4 + 1], // tile index
                        self.oam[i * 4 + 2], // attributes
                        self.oam[i * 4 + 3], // X
                    );
                    self.sprite_count += 1;
                } else {
                    // Sprite overflow
                    self.status |= 0x20;
                    break;
                }
            }
        }
    }

    /// Pre-fetch 33 background tiles into the scanline buffer.
    fn fill_bg_scanline_buffer(&mut self, mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        let pattern_base: u16 = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0x0000 };
        let fine_y = (self.v >> 12) & 0x07;
        let saved_v = self.v;

        for tile in 0..33 {
            let v = self.v;
            // Nametable byte
            let nt_addr = 0x2000 | (v & 0x0FFF);
            let tile_idx = self.ppu_read(nt_addr, mapper);

            // Attribute byte
            let attr_addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
            let attr = self.ppu_read(attr_addr, mapper);
            let shift = ((v >> 4) & 0x04) | (v & 0x02);
            let palette_idx = ((attr >> shift) & 0x03) as u8;

            // Pattern table planes
            let tile_addr = pattern_base + tile_idx as u16 * 16 + fine_y;
            let plane0 = self.ppu_read(tile_addr, mapper);
            let plane1 = self.ppu_read(tile_addr + 8, mapper);

            // Decode 8 pixels
            let offset = tile * 8;
            for bit in 0..8 {
                let color = ((plane0 >> (7 - bit)) & 1) | (((plane1 >> (7 - bit)) & 1) << 1);
                self.bg_pixel_buffer[offset + bit] = color;
                self.bg_palette_buffer[offset + bit] = palette_idx;
            }

            self.increment_x();
        }

        self.v = saved_v;
    }

    /// Pre-compute sprite pixels into scanline buffers.
    fn fill_sprite_scanline_buffer(&mut self, mapper: &mut dyn crate::cartridge::mapper::Mapper) {
        self.sprite_pixel_buffer = [0; 256];
        self.sprite_palette_buffer = [0; 256];
        self.sprite_priority_buffer = [false; 256];
        self.sprite_zero_buffer = [false; 256];

        let sprite_height: i16 = if self.ctrl & 0x20 != 0 { 16 } else { 8 };

        // Iterate in reverse so lower-index sprites overwrite higher-index (priority)
        for i in (0..self.sprite_count as usize).rev() {
            let (sp_y, sp_tile, sp_attr, sp_x) = self.sprite_scanline[i];
            let flip_h = sp_attr & 0x40 != 0;
            let flip_v = sp_attr & 0x80 != 0;
            let palette_idx = (sp_attr & 0x03) + 4;
            let priority = sp_attr & 0x20 != 0;

            let mut row = self.scanline - sp_y as i16;
            if flip_v {
                row = sprite_height - 1 - row;
            }

            let (tile_addr, tile_row) = if sprite_height == 16 {
                let bank = (sp_tile & 0x01) as u16 * 0x1000;
                let tile_num = sp_tile & 0xFE;
                if row < 8 {
                    (bank + tile_num as u16 * 16, row as u16)
                } else {
                    (bank + (tile_num + 1) as u16 * 16, (row - 8) as u16)
                }
            } else {
                let pattern_base: u16 = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
                (pattern_base + sp_tile as u16 * 16, row as u16)
            };

            let addr = tile_addr + tile_row;
            let plane0 = self.ppu_read(addr, mapper);
            let plane1 = self.ppu_read(addr + 8, mapper);

            for col in 0..8u16 {
                let x = sp_x as u16 + col;
                if x >= 256 {
                    continue;
                }
                let bit = if flip_h { col } else { 7 - col };
                let color = ((plane0 >> bit) & 1) | (((plane1 >> bit) & 1) << 1);
                if color != 0 {
                    self.sprite_pixel_buffer[x as usize] = color;
                    self.sprite_palette_buffer[x as usize] = palette_idx;
                    self.sprite_priority_buffer[x as usize] = priority;
                    if i == 0 && self.sprite_zero_on_line {
                        self.sprite_zero_buffer[x as usize] = true;
                    }
                }
            }
        }
    }
}
