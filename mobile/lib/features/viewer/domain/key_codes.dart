import 'input_event.dart';

/// USB HID usage codes (Keyboard/Keypad page, 0x07) for named keys. These match
/// the codes the Rust agent decodes in `desksync-input`'s `map_hid_key`.
abstract final class HidKey {
  /// Return/Enter.
  static const enter = 0x28;

  /// Escape.
  static const escape = 0x29;

  /// Backspace.
  static const backspace = 0x2A;

  /// Tab.
  static const tab = 0x2B;

  /// Space.
  static const space = 0x2C;
}

/// A HID code paired with whether Shift must be held to produce the character.
class HidChar {
  /// Creates a HID char mapping.
  const HidChar(this.code, {this.shift = false});

  /// The HID usage code of the base key.
  final int code;

  /// Whether Shift is required.
  final bool shift;
}

/// Map a single character to the HID key (and shift state) that produces it on
/// a US layout. Returns null for characters we do not map. Letters map to their
/// base key with Shift set for uppercase.
HidChar? hidForChar(String ch) {
  if (ch.length != 1) return null;
  final c = ch.codeUnitAt(0);

  // a-z
  if (c >= 0x61 && c <= 0x7A) return HidChar(0x04 + (c - 0x61));
  // A-Z -> same base key + shift
  if (c >= 0x41 && c <= 0x5A) return HidChar(0x04 + (c - 0x41), shift: true);
  // 1-9
  if (c >= 0x31 && c <= 0x39) return HidChar(0x1E + (c - 0x31));
  // 0
  if (ch == '0') return const HidChar(0x27);

  switch (ch) {
    case ' ':
      return const HidChar(HidKey.space);
    case '\n':
      return const HidChar(HidKey.enter);
    case '\t':
      return const HidChar(HidKey.tab);
    case '-':
      return const HidChar(0x2D);
    case '_':
      return const HidChar(0x2D, shift: true);
    case '=':
      return const HidChar(0x2E);
    case '+':
      return const HidChar(0x2E, shift: true);
    case '[':
      return const HidChar(0x2F);
    case '{':
      return const HidChar(0x2F, shift: true);
    case ']':
      return const HidChar(0x30);
    case '}':
      return const HidChar(0x30, shift: true);
    case '\\':
      return const HidChar(0x31);
    case '|':
      return const HidChar(0x31, shift: true);
    case ';':
      return const HidChar(0x33);
    case ':':
      return const HidChar(0x33, shift: true);
    case '\'':
      return const HidChar(0x34);
    case '"':
      return const HidChar(0x34, shift: true);
    case '`':
      return const HidChar(0x35);
    case '~':
      return const HidChar(0x35, shift: true);
    case ',':
      return const HidChar(0x36);
    case '<':
      return const HidChar(0x36, shift: true);
    case '.':
      return const HidChar(0x37);
    case '>':
      return const HidChar(0x37, shift: true);
    case '/':
      return const HidChar(0x38);
    case '?':
      return const HidChar(0x38, shift: true);
    case '!':
      return const HidChar(0x1E, shift: true);
    case '@':
      return const HidChar(0x1F, shift: true);
    case '#':
      return const HidChar(0x20, shift: true);
    case '\$':
      return const HidChar(0x21, shift: true);
    case '%':
      return const HidChar(0x22, shift: true);
    case '^':
      return const HidChar(0x23, shift: true);
    case '&':
      return const HidChar(0x24, shift: true);
    case '*':
      return const HidChar(0x25, shift: true);
    case '(':
      return const HidChar(0x26, shift: true);
    case ')':
      return const HidChar(0x27, shift: true);
    default:
      return null;
  }
}

/// Build the press+release [KeyEvent]s for a single typed character, applying
/// Shift when required. Returns an empty list for unmapped characters.
List<KeyEvent> keyEventsForChar(String ch) {
  final mapped = hidForChar(ch);
  if (mapped == null) return const [];
  final mods = Modifiers(shift: mapped.shift);
  return [
    KeyEvent(code: mapped.code, pressed: true, modifiers: mods),
    KeyEvent(code: mapped.code, pressed: false, modifiers: mods),
  ];
}

/// Build press+release events for a named key (already a HID code).
List<KeyEvent> keyEventsForCode(int code) => [
      KeyEvent(code: code, pressed: true),
      KeyEvent(code: code, pressed: false),
    ];
