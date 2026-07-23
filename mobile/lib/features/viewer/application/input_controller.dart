import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../domain/input_event.dart';
import '../domain/key_codes.dart';
import '../domain/touch_mapping.dart';
import 'input_sink.dart';

/// Translates high-level viewer gestures/keystrokes into [InputEvent]s and
/// dispatches them to the active [InputSink]. Its state is the running count of
/// events sent, which the UI can surface for debugging and tests can assert on.
class InputController extends Notifier<int> {
  InputSink get _sink => ref.read(inputSinkProvider);

  @override
  int build() => 0;

  void _dispatch(List<InputEvent> events) {
    if (events.isEmpty) return;
    _sink.sendAll(events);
    state += events.length;
  }

  /// Tap/click at a normalized point.
  void click(
    NormalizedPoint p, {
    PointerButton button = PointerButton.left,
    Modifiers modifiers = Modifiers.none,
  }) {
    _dispatch(TouchMapping.click(p, button: button, modifiers: modifiers));
  }

  /// Move the pointer to a normalized point (hover/drag update).
  void move(NormalizedPoint p) => _dispatch([TouchMapping.move(p)]);

  /// Begin a drag: move to the start point, then press the button.
  void dragStart(NormalizedPoint p, {PointerButton button = PointerButton.left}) {
    _dispatch([TouchMapping.move(p), TouchMapping.buttonDown(p, button: button)]);
  }

  /// Continue a drag by moving the pointer.
  void dragUpdate(NormalizedPoint p) => _dispatch([TouchMapping.move(p)]);

  /// End a drag by releasing the button.
  void dragEnd({PointerButton button = PointerButton.left}) {
    _dispatch([TouchMapping.buttonUp(button: button)]);
  }

  /// Scroll by pixel deltas.
  void scroll(double dxPixels, double dyPixels) {
    _dispatch([TouchMapping.scroll(dxPixels, dyPixels)]);
  }

  /// Type a string as a sequence of key press/release events.
  void typeText(String text) {
    final events = <InputEvent>[];
    for (final rune in text.runes) {
      events.addAll(keyEventsForChar(String.fromCharCode(rune)));
    }
    _dispatch(events);
  }

  /// Send a single named key (already a HID usage code), press then release.
  void sendKey(int hidCode) => _dispatch(keyEventsForCode(hidCode));

  /// Set the remote clipboard text.
  void setClipboard(String text) =>
      _dispatch([ClipboardTextEvent(text: text)]);
}

/// Provides the [InputController].
final inputControllerProvider =
    NotifierProvider<InputController, int>(InputController.new);
