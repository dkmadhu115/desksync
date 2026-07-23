import 'dart:convert';

import 'package:desksync_mobile/features/viewer/application/data_channel_input_sink.dart';
import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('serializes each event to a JSON text frame', () {
    final frames = <String>[];
    final sink = DataChannelInputSink(frames.add);

    sink.send(const MouseMoveEvent(x: 0.25, y: 0.5));
    sink.send(const MouseButtonEvent(button: PointerButton.right, pressed: true));

    expect(sink.count, 2);
    expect(jsonDecode(frames[0]), {'type': 'mouse_move', 'x': 0.25, 'y': 0.5});
    final btn = jsonDecode(frames[1]) as Map<String, dynamic>;
    expect(btn['type'], 'mouse_button');
    expect(btn['button'], 'right');
    expect(btn['pressed'], true);
  });

  test('sendAll forwards events in order', () {
    final frames = <String>[];
    final sink = DataChannelInputSink(frames.add);

    sink.sendAll(const [
      KeyEvent(code: 4, pressed: true),
      KeyEvent(code: 4, pressed: false),
    ]);

    expect(frames.length, 2);
    expect((jsonDecode(frames[0]) as Map)['pressed'], true);
    expect((jsonDecode(frames[1]) as Map)['pressed'], false);
  });
}
