import 'dart:convert';

import '../domain/input_event.dart';
import 'input_sink.dart';

/// An [InputSink] that serializes events to JSON and forwards each frame over
/// the WebRTC data channel. It is decoupled from `flutter_webrtc` via a plain
/// send callback so the serialization is unit-testable without a peer
/// connection; `WebRtcSession` supplies a callback that writes to the channel.
class DataChannelInputSink implements InputSink {
  /// Creates a sink over the given text-frame [_send] callback.
  DataChannelInputSink(this._send);

  final void Function(String jsonFrame) _send;
  int _count = 0;

  /// Number of events sent so far.
  int get count => _count;

  @override
  void send(InputEvent event) {
    _count++;
    _send(jsonEncode(event.toJson()));
  }

  @override
  void sendAll(Iterable<InputEvent> events) {
    for (final e in events) {
      send(e);
    }
  }
}
