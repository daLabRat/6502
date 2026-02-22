/// Superset of all input buttons across all supported systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    // D-pad / directional
    Up,
    Down,
    Left,
    Right,
    // NES / generic gamepad
    A,
    B,
    Start,
    Select,
    // Fire buttons (Atari 2600, etc.)
    Fire,
    // Keyboard key (Apple II, C64) - stores ASCII value
    Key(u8),
}

/// An input event: a button was either pressed or released.
#[derive(Debug, Clone, Copy)]
pub struct InputEvent {
    pub button: Button,
    pub pressed: bool,
    /// Controller port (0 = player 1, 1 = player 2)
    pub port: u8,
}
