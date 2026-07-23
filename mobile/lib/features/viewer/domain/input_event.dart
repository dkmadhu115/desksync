/// Client-side model of the input events sent to the desktop agent.
///
/// The JSON produced here MUST match, byte-for-byte in structure, the Rust
/// agent's `InputEvent` enum (`desktop-agent/crates/input`), which is tagged
/// with `#[serde(tag = "type", rename_all = "snake_case")]`. Coordinates are
/// normalized to `[0,1]` relative to the remote display so they are
/// resolution-independent. In Phase 5 these are serialized onto the WebRTC data
/// channel; here they flow through an [InputSink].
library;

/// Modifier keys accompanying a key/pointer event. Serializes to
/// `{"ctrl":bool,"alt":bool,"shift":bool,"meta":bool}`.
class Modifiers {
  /// Creates a modifier set (all released by default).
  const Modifiers({
    this.ctrl = false,
    this.alt = false,
    this.shift = false,
    this.meta = false,
  });

  /// Control key held.
  final bool ctrl;

  /// Alt/Option key held.
  final bool alt;

  /// Shift key held.
  final bool shift;

  /// Command/Meta/Win key held.
  final bool meta;

  /// No modifiers held.
  static const none = Modifiers();

  /// JSON form.
  Map<String, dynamic> toJson() => {
        'ctrl': ctrl,
        'alt': alt,
        'shift': shift,
        'meta': meta,
      };
}

/// Mouse buttons. Serializes to the lowercase names `left`/`right`/`middle`.
enum PointerButton {
  /// Left button.
  left,

  /// Right button.
  right,

  /// Middle button.
  middle,
}

/// Base type for all input events. Each subtype serializes with a `type`
/// discriminator matching the agent's snake_case variant name.
sealed class InputEvent {
  const InputEvent();

  /// The wire representation.
  Map<String, dynamic> toJson();
}

/// Absolute pointer move to a normalized position.
class MouseMoveEvent extends InputEvent {
  /// Creates a move event.
  const MouseMoveEvent({required this.x, required this.y});

  /// Normalized X in [0,1].
  final double x;

  /// Normalized Y in [0,1].
  final double y;

  @override
  Map<String, dynamic> toJson() => {'type': 'mouse_move', 'x': x, 'y': y};
}

/// Pointer button press/release.
class MouseButtonEvent extends InputEvent {
  /// Creates a button event.
  const MouseButtonEvent({
    required this.button,
    required this.pressed,
    this.modifiers = Modifiers.none,
  });

  /// Which button.
  final PointerButton button;

  /// True on press, false on release.
  final bool pressed;

  /// Active modifiers.
  final Modifiers modifiers;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'mouse_button',
        'button': button.name,
        'pressed': pressed,
        'modifiers': modifiers.toJson(),
      };
}

/// Scroll wheel / trackpad scroll.
class ScrollEvent extends InputEvent {
  /// Creates a scroll event.
  const ScrollEvent({required this.dx, required this.dy});

  /// Horizontal delta.
  final double dx;

  /// Vertical delta.
  final double dy;

  @override
  Map<String, dynamic> toJson() => {'type': 'scroll', 'dx': dx, 'dy': dy};
}

/// Key press/release identified by a USB HID usage code.
class KeyEvent extends InputEvent {
  /// Creates a key event.
  const KeyEvent({
    required this.code,
    required this.pressed,
    this.modifiers = Modifiers.none,
  });

  /// USB HID usage code.
  final int code;

  /// True on press, false on release.
  final bool pressed;

  /// Active modifiers.
  final Modifiers modifiers;

  @override
  Map<String, dynamic> toJson() => {
        'type': 'key',
        'code': code,
        'pressed': pressed,
        'modifiers': modifiers.toJson(),
      };
}

/// Directly set the remote clipboard text.
class ClipboardTextEvent extends InputEvent {
  /// Creates a clipboard event.
  const ClipboardTextEvent({required this.text});

  /// UTF-8 clipboard contents.
  final String text;

  @override
  Map<String, dynamic> toJson() => {'type': 'clipboard_text', 'text': text};
}
