use super::Ppu;
use super::palette::NES_PALETTE;

impl Ppu {
    /// Render a single pixel at the current scanline/cycle position.
    /// Reads from pre-filled scanline buffers instead of doing per-pixel VRAM access.
    pub(crate) fn render_pixel(&mut self) {
        let x = (self.cycle - 1) as usize;
        let y = self.scanline as u32;

        if x >= 256 || y >= 240 {
            return;
        }

        // Background pixel from buffer (fine_x shifts into the 33-tile buffer)
        let (bg_color_idx, bg_palette_idx) = if self.mask & 0x08 != 0
            && (x >= 8 || self.mask & 0x02 != 0)
        {
            let buf_x = x + self.fine_x as usize;
            (self.bg_pixel_buffer[buf_x], self.bg_palette_buffer[buf_x])
        } else {
            (0, 0)
        };

        // Sprite pixel from buffer
        let (sp_color_idx, sp_palette_idx, sp_priority, sp_is_zero) =
            if self.mask & 0x10 != 0 && (x >= 8 || self.mask & 0x04 != 0) {
                (
                    self.sprite_pixel_buffer[x],
                    self.sprite_palette_buffer[x],
                    self.sprite_priority_buffer[x],
                    self.sprite_zero_buffer[x],
                )
            } else {
                (0, 0, false, false)
            };

        // Priority multiplexer
        let (palette, color_idx) = match (bg_color_idx != 0, sp_color_idx != 0) {
            (false, false) => (0u8, 0u8),
            (false, true) => (sp_palette_idx, sp_color_idx),
            (true, false) => (bg_palette_idx, bg_color_idx),
            (true, true) => {
                // Sprite 0 hit check: must not fire at x==255 or when left-clip is active
                if sp_is_zero
                    && x < 255
                    && !(x < 8 && (self.mask & 0x02 == 0 || self.mask & 0x04 == 0))
                {
                    self.status |= 0x40;
                }
                if sp_priority {
                    (bg_palette_idx, bg_color_idx)
                } else {
                    (sp_palette_idx, sp_color_idx)
                }
            }
        };

        // Look up palette RAM
        let palette_addr = if color_idx == 0 {
            0x3F00
        } else {
            0x3F00 | (palette as u16) << 2 | color_idx as u16
        };
        let nes_color = self.read_palette(palette_addr) & 0x3F;
        let rgb = NES_PALETTE[nes_color as usize];

        self.framebuffer.set_pixel_rgb(x as u32, y, rgb);
    }
}
