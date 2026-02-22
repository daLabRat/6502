use emu_common::FrameBuffer;
use crate::memory::Memory;
use crate::soft_switch::SoftSwitches;

/// Hi-res screen line addresses (non-linear, same interleaving as text).
fn hires_line_addr(line: usize, page2: bool) -> u16 {
    let base: u16 = if page2 { 0x4000 } else { 0x2000 };
    let group = line / 64;
    let subgroup = (line % 64) / 8;
    let row_in_group = line % 8;
    base + (row_in_group as u16) * 0x400
        + (subgroup as u16) * 0x80
        + (group as u16) * 0x28
}

/// Apple II hi-res colors.
/// The color depends on the column (even/odd) and the high bit of the byte.
static HIRES_COLORS: [[u32; 2]; 2] = [
    [0xDD22DD, 0x11DD00], // group 0: purple, green
    [0x2222FF, 0xFF6600], // group 1: blue, orange
];

/// Display width for bounds checking.
const WIDTH: u32 = 560;

/// Render hi-res mode (560x192, each pixel doubled horizontally).
pub fn render_hires(
    fb: &mut FrameBuffer,
    memory: &Memory,
    switches: &SoftSwitches,
    max_scanline: u32,
) {
    for line in 0..max_scanline.min(192) {
        let addr = hires_line_addr(line as usize, switches.page2);

        for col in 0..40u16 {
            let byte = memory.ram[(addr + col) as usize];
            let color_group = if byte & 0x80 != 0 { 1 } else { 0 };

            for bit in 0..7 {
                let x = (col as u32 * 7 + bit) * 2;
                if x >= WIDTH { continue; }

                let pixel_on = (byte >> bit) & 1 != 0;
                let color = if pixel_on {
                    let is_odd = ((col as u32 * 7 + bit) % 2) == 1;
                    HIRES_COLORS[color_group][is_odd as usize]
                } else {
                    0x000000
                };

                fb.set_pixel_rgb(x, line, color);
                if x + 1 < WIDTH {
                    fb.set_pixel_rgb(x + 1, line, color);
                }
            }
        }
    }
}
