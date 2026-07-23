import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('InputEvent wire format (must match Rust agent serde)', () {
    test('mouse_move', () {
      expect(
        const MouseMoveEvent(x: 0.25, y: 0.75).toJson(),
        {'type': 'mouse_move', 'x': 0.25, 'y': 0.75},
      );
    });

    test('mouse_button with lowercase button name and modifiers', () {
      expect(
        const MouseButtonEvent(
          button: PointerButton.right,
          pressed: true,
          modifiers: Modifiers(ctrl: true),
        ).toJson(),
        {
          'type': 'mouse_button',
          'button': 'right',
          'pressed': true,
          'modifiers': {
            'ctrl': true,
            'alt': false,
            'shift': false,
            'meta': false,
          },
        },
      );
    });

    test('scroll', () {
      expect(
        const ScrollEvent(dx: 1.5, dy: -2.0).toJson(),
        {'type': 'scroll', 'dx': 1.5, 'dy': -2.0},
      );
    });

    test('key with modifiers', () {
      expect(
        const KeyEvent(code: 4, pressed: false, modifiers: Modifiers(shift: true))
            .toJson(),
        {
          'type': 'key',
          'code': 4,
          'pressed': false,
          'modifiers': {
            'ctrl': false,
            'alt': false,
            'shift': true,
            'meta': false,
          },
        },
      );
    });

    test('clipboard_text', () {
      expect(
        const ClipboardTextEvent(text: 'hello').toJson(),
        {'type': 'clipboard_text', 'text': 'hello'},
      );
    });

    test('button names are the lowercase variants the agent expects', () {
      expect(PointerButton.left.name, 'left');
      expect(PointerButton.right.name, 'right');
      expect(PointerButton.middle.name, 'middle');
    });
  });
}
