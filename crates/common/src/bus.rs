/// Memory bus trait - all system hardware sits behind this interface.
/// The CPU is generic over Bus, so each system provides its own implementation
/// that routes addresses to the appropriate hardware components.
pub trait Bus {
    /// Read a byte from the given address. May trigger side effects
    /// (e.g., reading PPU status register clears vblank flag).
    fn read(&mut self, addr: u16) -> u8;

    /// Write a byte to the given address.
    fn write(&mut self, addr: u16, val: u8);

    /// Side-effect-free read for debug tools. Returns the same value
    /// as read() but without modifying any internal state.
    fn peek(&self, addr: u16) -> u8;

    /// Called by the CPU after each instruction with the number of cycles
    /// that instruction consumed. The bus implementation uses this to
    /// advance other system components (PPU, APU, timers, etc.).
    fn tick(&mut self, cycles: u8);

    /// Check if an NMI is pending and clear it.
    fn poll_nmi(&mut self) -> bool;

    /// Check if an IRQ line is asserted (active low, level-triggered).
    fn poll_irq(&mut self) -> bool;

    /// Check if the SO (Set Overflow) pin has been pulsed low since the last call.
    /// On the NMOS 6502, a falling edge on SO sets the V flag.
    /// Used by the 1541 drive to signal BYTE READY to the CPU.
    fn poll_so(&mut self) -> bool {
        false
    }
}
