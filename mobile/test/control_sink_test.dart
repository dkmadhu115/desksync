import 'package:desksync_mobile/features/devtools/application/control_sink.dart';
import 'package:flutter_test/flutter_test.dart';

class _RecordingSink implements ControlSink {
  final frames = <String>[];

  @override
  void send(String jsonFrame) => frames.add(jsonFrame);
}

void main() {
  group('SwitchableControlSink', () {
    test('forwards to the attached target', () {
      final target = _RecordingSink();
      final sink = SwitchableControlSink()..attach(target);

      sink.send('{"a":1}');

      expect(target.frames, ['{"a":1}']);
      expect(sink.droppedFromWire, 0);
      expect(sink.hasTarget, isTrue);
    });

    test('drops and counts frames when no target attached', () {
      final sink = SwitchableControlSink();

      sink.send('{"a":1}');
      sink.send('{"b":2}');

      expect(sink.hasTarget, isFalse);
      expect(sink.droppedFromWire, 2);
    });

    test('detach stops forwarding', () {
      final target = _RecordingSink();
      final sink = SwitchableControlSink()..attach(target);
      sink.send('first');
      sink.detach();
      sink.send('second');

      expect(target.frames, ['first']);
      expect(sink.droppedFromWire, 1);
    });
  });
}
