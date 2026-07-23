import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:desksync_mobile/features/viewer/domain/touch_mapping.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('normalize', () {
    test('maps corners and center', () {
      expect(TouchMapping.normalize(0, 0, 400, 300).x, 0);
      expect(TouchMapping.normalize(0, 0, 400, 300).y, 0);

      final center = TouchMapping.normalize(200, 150, 400, 300);
      expect(center.x, 0.5);
      expect(center.y, 0.5);

      final br = TouchMapping.normalize(400, 300, 400, 300);
      expect(br.x, 1.0);
      expect(br.y, 1.0);
    });

    test('clamps out-of-bounds positions to [0,1]', () {
      final p = TouchMapping.normalize(-50, 9999, 400, 300);
      expect(p.x, 0.0);
      expect(p.y, 1.0);
    });

    test('is safe for a zero-sized surface', () {
      final p = TouchMapping.normalize(10, 10, 0, 0);
      expect(p.x, 0.0);
      expect(p.y, 0.0);
    });
  });

  group('click', () {
    test('produces move + press + release in order', () {
      final events = TouchMapping.click(const NormalizedPoint(0.5, 0.5));
      expect(events, hasLength(3));
      expect(events[0], isA<MouseMoveEvent>());
      expect(events[1], isA<MouseButtonEvent>());
      expect((events[1] as MouseButtonEvent).pressed, isTrue);
      expect((events[2] as MouseButtonEvent).pressed, isFalse);
    });

    test('honours the requested button', () {
      final events = TouchMapping.click(
        const NormalizedPoint(0.1, 0.1),
        button: PointerButton.right,
      );
      expect((events[1] as MouseButtonEvent).button, PointerButton.right);
    });
  });

  group('scroll', () {
    test('negates vertical delta so drag-up scrolls down', () {
      final e = TouchMapping.scroll(0, 100, scale: 0.1);
      expect(e.dy, -10.0);
      expect(e.dx, 0.0);
    });
  });
}
