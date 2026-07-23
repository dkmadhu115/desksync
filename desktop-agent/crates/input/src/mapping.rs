//! Pure, platform-independent input mapping.
//!
//! The mobile client sends resolution-independent events: pointer coordinates
//! normalized to `[0,1]` and keys identified by their **USB HID usage code**.
//! This module converts those into concrete pixel coordinates and a
//! backend-neutral [`PhysicalKey`]. Keeping this logic pure means the
//! coordinate math and key table are fully unit-tested without touching the OS;
//! the `native` backend only has to translate [`PhysicalKey`] into its own
//! enum.

/// Map a normalized coordinate in `[0,1]` to an absolute pixel position on a
/// `width`×`height` display. Inputs are clamped so a malformed event can never
/// drive the cursor off-screen.
#[must_use]
pub fn normalized_to_pixel(x: f64, y: f64, width: u32, height: u32) -> (i32, i32) {
    let cx = x.clamp(0.0, 1.0);
    let cy = y.clamp(0.0, 1.0);
    let max_x = width.saturating_sub(1) as f64;
    let max_y = height.saturating_sub(1) as f64;
    ((cx * max_x).round() as i32, (cy * max_y).round() as i32)
}

/// A backend-neutral key: either a printable character or a named non-printing
/// key. The native backend maps this onto its own key enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalKey {
    /// A printable character (letters, digits, punctuation).
    Char(char),
    /// A named non-printing key.
    Named(NamedKey),
}

/// Named, non-printing keys we support for injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum NamedKey {
    Enter,
    Escape,
    Backspace,
    Tab,
    Space,
    CapsLock,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    ArrowRight,
    ArrowLeft,
    ArrowDown,
    ArrowUp,
    F(u8),
}

/// Translate a USB HID usage code (Keyboard/Keypad page, 0x07) into a
/// [`PhysicalKey`]. Returns `None` for codes we do not map. Letters are
/// returned lowercase; the caller applies Shift for capitalization.
#[must_use]
pub fn map_hid_key(code: u32) -> Option<PhysicalKey> {
    use NamedKey::*;
    use PhysicalKey::{Char, Named};

    let key = match code {
        // a..z -> HID 0x04..=0x1D
        0x04..=0x1D => Char((b'a' + (code - 0x04) as u8) as char),
        // 1..9 -> HID 0x1E..=0x26, 0 -> HID 0x27
        0x1E..=0x26 => Char((b'1' + (code - 0x1E) as u8) as char),
        0x27 => Char('0'),
        0x28 => Named(Enter),
        0x29 => Named(Escape),
        0x2A => Named(Backspace),
        0x2B => Named(Tab),
        0x2C => Named(Space),
        0x2D => Char('-'),
        0x2E => Char('='),
        0x2F => Char('['),
        0x30 => Char(']'),
        0x31 => Char('\\'),
        0x33 => Char(';'),
        0x34 => Char('\''),
        0x35 => Char('`'),
        0x36 => Char(','),
        0x37 => Char('.'),
        0x38 => Char('/'),
        0x39 => Named(CapsLock),
        // F1..F12 -> HID 0x3A..=0x45
        0x3A..=0x45 => Named(F((code - 0x3A + 1) as u8)),
        0x4C => Named(Delete),
        0x4A => Named(Home),
        0x4D => Named(End),
        0x4B => Named(PageUp),
        0x4E => Named(PageDown),
        0x4F => Named(ArrowRight),
        0x50 => Named(ArrowLeft),
        0x51 => Named(ArrowDown),
        0x52 => Named(ArrowUp),
        _ => return None,
    };
    Some(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_and_center_map_correctly() {
        assert_eq!(normalized_to_pixel(0.0, 0.0, 1920, 1080), (0, 0));
        assert_eq!(normalized_to_pixel(1.0, 1.0, 1920, 1080), (1919, 1079));
        assert_eq!(normalized_to_pixel(0.5, 0.5, 101, 101), (50, 50));
    }

    #[test]
    fn out_of_range_is_clamped() {
        assert_eq!(normalized_to_pixel(-1.0, 2.0, 100, 100), (0, 99));
        assert_eq!(normalized_to_pixel(5.0, -5.0, 100, 100), (99, 0));
    }

    #[test]
    fn zero_sized_display_is_safe() {
        assert_eq!(normalized_to_pixel(0.5, 0.5, 0, 0), (0, 0));
    }

    #[test]
    fn maps_letters_digits_and_named_keys() {
        assert_eq!(map_hid_key(0x04), Some(PhysicalKey::Char('a')));
        assert_eq!(map_hid_key(0x1D), Some(PhysicalKey::Char('z')));
        assert_eq!(map_hid_key(0x1E), Some(PhysicalKey::Char('1')));
        assert_eq!(map_hid_key(0x27), Some(PhysicalKey::Char('0')));
        assert_eq!(map_hid_key(0x28), Some(PhysicalKey::Named(NamedKey::Enter)));
        assert_eq!(map_hid_key(0x2C), Some(PhysicalKey::Named(NamedKey::Space)));
        assert_eq!(map_hid_key(0x4F), Some(PhysicalKey::Named(NamedKey::ArrowRight)));
    }

    #[test]
    fn maps_function_keys() {
        assert_eq!(map_hid_key(0x3A), Some(PhysicalKey::Named(NamedKey::F(1))));
        assert_eq!(map_hid_key(0x45), Some(PhysicalKey::Named(NamedKey::F(12))));
    }

    #[test]
    fn unknown_codes_return_none() {
        assert_eq!(map_hid_key(0x00), None);
        assert_eq!(map_hid_key(0xFFFF), None);
    }
}
