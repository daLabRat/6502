use crate::debug::{CpuDebugState, DebugSection};
use crate::framebuffer::FrameBuffer;
use crate::input::InputEvent;
use crate::audio::AudioSample;

/// Trait that all system emulators implement. The frontend interacts
/// with every system through this uniform interface.
pub trait SystemEmulator {
    /// Advance the emulation by one video frame (e.g., ~29780 CPU cycles for NTSC NES).
    /// Returns the number of audio samples generated this frame.
    fn step_frame(&mut self) -> usize;

    /// Get a reference to the current framebuffer for display.
    fn framebuffer(&self) -> &FrameBuffer;

    /// Drain generated audio samples into the provided buffer.
    /// Returns the number of samples written.
    fn audio_samples(&mut self, out: &mut [AudioSample]) -> usize;

    /// Send an input event to the system.
    fn handle_input(&mut self, event: InputEvent);

    /// Reset the system (power cycle).
    fn reset(&mut self);

    /// The native display width of this system.
    fn display_width(&self) -> u32;

    /// The native display height of this system.
    fn display_height(&self) -> u32;

    /// Display aspect ratio (width / height) for correct rendering.
    /// Most retro systems output non-square pixels onto 4:3 CRTs.
    /// Defaults to 4:3. Override only if the system uses a different display ratio.
    fn display_aspect_ratio(&self) -> f64 {
        4.0 / 3.0
    }

    /// Target frames per second (e.g., 60 for NTSC, 50 for PAL).
    fn target_fps(&self) -> f64 {
        60.0
    }

    /// Set the audio output sample rate. Called by the frontend after
    /// initializing the audio device so the emulator generates samples
    /// at the correct rate (device may be 48000 Hz, not 44100).
    fn set_sample_rate(&mut self, _rate: u32) {}

    /// Name of this system for display in the UI.
    fn system_name(&self) -> &str;

    /// Stable short identifier used for save-state directory names.
    /// Must not contain spaces or special characters.
    /// Defaults to `system_name()` but should be overridden by each system
    /// so that renaming the display name never invalidates existing save files.
    fn save_state_system_id(&self) -> &str {
        self.system_name()
    }

    // ── Debugger interface ────────────────────────────────────────────────

    /// Snapshot of CPU registers for the debugger.
    fn cpu_state(&self) -> CpuDebugState { CpuDebugState::default() }

    /// Side-effect-free read of the CPU address space.
    fn peek_memory(&self, addr: u16) -> u8 { let _ = addr; 0 }

    /// Disassemble one instruction at `addr`.
    /// Returns (formatted string, address of next instruction).
    fn disassemble(&self, addr: u16) -> (String, u16) { ("???".into(), addr.wrapping_add(1)) }

    /// Execute exactly one CPU instruction.
    fn step_instruction(&mut self) {}

    /// System-specific debug panels (VIC-II, SID, CIA, PPU, etc.).
    /// Returns named sections of key→value rows; the debugger renders them generically.
    fn system_debug_panels(&self) -> Vec<DebugSection> { vec![] }

    // ── Save state interface ──────────────────────────────────────────────

    /// Serialize the complete emulator state to a byte blob.
    /// Returns Err if save states are not supported for this system.
    fn save_state(&self) -> Result<Vec<u8>, String> {
        Err("Save states not supported for this system".into())
    }

    /// Restore emulator state from a byte blob previously returned by `save_state`.
    /// Returns Err on version mismatch, data corruption, or unsupported system.
    fn load_state(&mut self, data: &[u8]) -> Result<(), String> {
        let _ = data;
        Err("Save states not supported for this system".into())
    }

    /// Returns true if this system implements save/load state.
    fn supports_save_states(&self) -> bool { false }

    /// If the system has a modified disk image (e.g. Disk II writes), return
    /// the new image bytes and clear the dirty flag. Returns `None` otherwise.
    fn take_modified_disk_image(&mut self) -> Option<Vec<u8>> { None }
}
