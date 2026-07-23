import 'package:desksync_mobile/features/viewer/domain/key_codes.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('hidForChar (matches the Rust agent HID table)', () {
    test('lowercase letters map to 0x04..0x1D without shift', () {
      expect(hidForChar('a')!.code, 0x04);
      expect(hidForChar('a')!.shift, isFalse);
      expect(hidForChar('z')!.code, 0x1D);
    });

    test('uppercase letters use the same base code plus shift', () {
      expect(hidForChar('A')!.code, 0x04);
      expect(hidForChar('A')!.shift, isTrue);
    });

    test('digits map to 0x1E..0x27', () {
      expect(hidForChar('1')!.code, 0x1E);
      expect(hidForChar('9')!.code, 0x26);
      expect(hidForChar('0')!.code, 0x27);
    });

    test('space maps to the space HID code', () {
      expect(hidForChar(' ')!.code, HidKey.space);
    });

    test('shifted symbols carry the shift flag', () {
      expect(hidForChar('!')!.code, 0x1E);
      expect(hidForChar('!')!.shift, isTrue);
      expect(hidForChar('_')!.code, 0x2D);
      expect(hidForChar('_')!.shift, isTrue);
    });

    test('unmapped characters return null', () {
      expect(hidForChar('€'), isNull);
      expect(hidForChar('ab'), isNull);
    });
  });

  group('keyEventsForChar', () {
    test('emits press then release with the right modifiers', () {
      final events = keyEventsForChar('A');
      expect(events, hasLength(2));
      expect(events[0].pressed, isTrue);
      expect(events[0].modifiers.shift, isTrue);
      expect(events[1].pressed, isFalse);
      expect(events[0].code, 0x04);
    });

    test('returns empty for unmapped characters', () {
      expect(keyEventsForChar('€'), isEmpty);
    });
  });

  test('keyEventsForCode emits a press/release pair', () {
    final events = keyEventsForCode(HidKey.enter);
    expect(events.map((e) => e.pressed), [true, false]);
    expect(events.first.code, HidKey.enter);
  });
}
