use egui::Key;
use emu_common::{Button, InputEvent};

/// Default keyboard mapping for NES/generic:
/// Arrows = D-pad, Z = B, X = A, Enter = Start, RShift = Select
pub fn map_key_to_button(key: Key) -> Option<Button> {
    match key {
        Key::ArrowUp => Some(Button::Up),
        Key::ArrowDown => Some(Button::Down),
        Key::ArrowLeft => Some(Button::Left),
        Key::ArrowRight => Some(Button::Right),
        Key::Z => Some(Button::B),
        Key::X => Some(Button::A),
        Key::Enter => Some(Button::Start),
        Key::Backspace => Some(Button::Select),
        Key::Space => Some(Button::Fire),
        _ => None,
    }
}

/// Process egui input events and return a list of InputEvents.
pub fn process_egui_input(ctx: &egui::Context) -> Vec<InputEvent> {
    let mut events = Vec::new();

    ctx.input(|input| {
        for event in &input.events {
            match event {
                egui::Event::Key { key, pressed, modifiers, .. } => {
                    if let Some(button) = map_key_to_button(*key) {
                        events.push(InputEvent {
                            button,
                            pressed: *pressed,
                            port: 0,
                        });
                    }
                    // Ctrl+letter → control character (Ctrl+C = $03, etc.)
                    if modifiers.ctrl {
                        if let Some(ctrl_code) = key_to_ctrl(*key) {
                            events.push(InputEvent {
                                button: Button::Key(ctrl_code),
                                pressed: *pressed,
                                port: 0,
                            });
                        }
                    } else if let Some(c) = key_to_ascii(*key) {
                        // Keyboard keys for Apple II / C64 (send both press and release)
                        events.push(InputEvent {
                            button: Button::Key(c),
                            pressed: *pressed,
                            port: 0,
                        });
                    }
                }
                // Text events capture shifted/special characters (", !, @, etc.)
                // Only send press — release is handled by the emulator via delayed
                // key-up, since Text events deliver press+release in the same frame
                // and keyboard matrix scanners (C64 CIA) would never see the key.
                egui::Event::Text(text) => {
                    for ch in text.chars() {
                        if let Some(ascii) = char_to_apple_ascii(ch) {
                            events.push(InputEvent {
                                button: Button::Key(ascii),
                                pressed: true,
                                port: 0,
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    });

    events
}

/// Convert a typed character to Apple II ASCII (uppercase).
/// Only returns values for characters NOT already handled by key_to_ascii
/// (shifted symbols like ", !, @, #, $, etc.) to avoid duplicate events.
fn char_to_apple_ascii(ch: char) -> Option<u8> {
    let c = ch as u32;
    if c < 0x20 || c > 0x7E { return None; }
    let ascii = if c >= 0x61 && c <= 0x7A { c as u8 - 0x20 } else { c as u8 };
    // Skip characters already handled by key_to_ascii (A-Z, 0-9, space)
    match ascii {
        b'A'..=b'Z' | b'0'..=b'9' | b' ' => None,
        _ => Some(ascii),
    }
}

/// Map Ctrl+letter to control character code.
/// Ctrl+A = $01, Ctrl+B = $02, ..., Ctrl+Z = $1A
/// Ctrl+C ($03) is the Apple II BASIC break key.
fn key_to_ctrl(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(0x01), Key::B => Some(0x02), Key::C => Some(0x03),
        Key::D => Some(0x04), Key::E => Some(0x05), Key::F => Some(0x06),
        Key::G => Some(0x07), Key::H => Some(0x08), Key::I => Some(0x09),
        Key::J => Some(0x0A), Key::K => Some(0x0B), Key::L => Some(0x0C),
        Key::M => Some(0x0D), Key::N => Some(0x0E), Key::O => Some(0x0F),
        Key::P => Some(0x10), Key::Q => Some(0x11), Key::R => Some(0x12),
        Key::S => Some(0x13), Key::T => Some(0x14), Key::U => Some(0x15),
        Key::V => Some(0x16), Key::W => Some(0x17), Key::X => Some(0x18),
        Key::Y => Some(0x19), Key::Z => Some(0x1A),
        _ => None,
    }
}

/// Map egui Key to ASCII for keyboard-based systems.
fn key_to_ascii(key: Key) -> Option<u8> {
    match key {
        Key::A => Some(b'A'),
        Key::B => Some(b'B'),
        Key::C => Some(b'C'),
        Key::D => Some(b'D'),
        Key::E => Some(b'E'),
        Key::F => Some(b'F'),
        Key::G => Some(b'G'),
        Key::H => Some(b'H'),
        Key::I => Some(b'I'),
        Key::J => Some(b'J'),
        Key::K => Some(b'K'),
        Key::L => Some(b'L'),
        Key::M => Some(b'M'),
        Key::N => Some(b'N'),
        Key::O => Some(b'O'),
        Key::P => Some(b'P'),
        Key::Q => Some(b'Q'),
        Key::R => Some(b'R'),
        Key::S => Some(b'S'),
        Key::T => Some(b'T'),
        Key::U => Some(b'U'),
        Key::V => Some(b'V'),
        Key::W => Some(b'W'),
        Key::X => Some(b'X'),
        Key::Y => Some(b'Y'),
        Key::Z => Some(b'Z'),
        Key::Num0 => Some(b'0'),
        Key::Num1 => Some(b'1'),
        Key::Num2 => Some(b'2'),
        Key::Num3 => Some(b'3'),
        Key::Num4 => Some(b'4'),
        Key::Num5 => Some(b'5'),
        Key::Num6 => Some(b'6'),
        Key::Num7 => Some(b'7'),
        Key::Num8 => Some(b'8'),
        Key::Num9 => Some(b'9'),
        Key::Space => Some(b' '),
        Key::Enter => Some(0x0D),
        Key::Escape => Some(0x1B),
        Key::Backspace => Some(0x08),
        // Apple IIe arrow key codes
        Key::ArrowLeft => Some(0x08),   // same as backspace
        Key::ArrowRight => Some(0x15),  // Ctrl+U / NAK
        Key::ArrowUp => Some(0x0B),     // Ctrl+K / VT
        Key::ArrowDown => Some(0x0A),   // Ctrl+J / LF
        _ => None,
    }
}
