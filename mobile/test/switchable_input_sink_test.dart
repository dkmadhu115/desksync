import 'package:desksync_mobile/features/viewer/application/input_sink.dart';
import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

void main() {
  const event = MouseMoveEvent(x: 0.5, y: 0.5);

  test('forwards to the fallback and counts drops when no target attached', () {
    final fallback = RecordingInputSink();
    final sink = SwitchableInputSink(fallback: fallback);

    expect(sink.hasTarget, isFalse);
    sink.send(event);

    expect(fallback.events, hasLength(1));
    expect(sink.droppedFromWire, 1);
  });

  test('forwards to the attached target and does not count drops', () {
    final fallback = RecordingInputSink();
    final target = RecordingInputSink();
    final sink = SwitchableInputSink(fallback: fallback)..attach(target);

    expect(sink.hasTarget, isTrue);
    sink.sendAll(const [event, event]);

    expect(target.events, hasLength(2));
    expect(fallback.events, isEmpty);
    expect(sink.droppedFromWire, 0);
  });

  test('detach routes subsequent events back to the fallback', () {
    final fallback = RecordingInputSink();
    final target = RecordingInputSink();
    final sink = SwitchableInputSink(fallback: fallback)..attach(target);

    sink.send(event);
    sink.detach();
    sink.send(event);

    expect(target.events, hasLength(1));
    expect(fallback.events, hasLength(1));
    expect(sink.hasTarget, isFalse);
  });
}
