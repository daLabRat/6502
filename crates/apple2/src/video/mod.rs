pub mod text;
pub mod hires;

use emu_common::FrameBuffer;
use crate::memory::Memory;
use crate::soft_switch::SoftSwitches;

/// Apple II display resolution: 560x192 (doubled horizontal for 80-column support).
pub const DISPLAY_WIDTH: u32 = 560;
pub const DISPLAY_HEIGHT: u32 = 192;

/// Render the Apple II display into a framebuffer.
/// `flash_on` toggles whether flashing characters ($40-$7F) display as inverse.
pub fn render(fb: &mut FrameBuffer, memory: &Memory, switches: &SoftSwitches, flash_on: bool) {
    if switches.text_mode {
        text::render_text(fb, memory, switches, flash_on);
    } else if switches.hires {
        if switches.mixed_mode {
            // Hi-res with text at bottom
            hires::render_hires(fb, memory, switches, 160);
            text::render_text_lines(fb, memory, switches, 20, 24, flash_on);
        } else {
            hires::render_hires(fb, memory, switches, 192);
        }
    } else {
        // Lo-res (render as text-mode colored blocks)
        text::render_lores(fb, memory, switches, flash_on);
    }
}
