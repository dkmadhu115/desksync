import 'dart:convert';
import 'dart:developer' as developer;

import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../domain/input_event.dart';

/// Destination for input events produced by the viewer.
///
/// Today this is a local logging sink; in Phase 5 the WebRTC data-channel
/// implementation replaces the provider override, so the viewer/controller code
/// does not change. Keeping it behind an interface also makes the input
/// pipeline trivially testable (a recording fake captures events).
abstract interface class InputSink {
  /// Send a single event.
  void send(InputEvent event);

  /// Send several events in order.
  void sendAll(Iterable<InputEvent> events);
}

/// Default sink that serializes events to JSON and logs them. Useful for
/// development and manual verification before the transport exists.
class LoggingInputSink implements InputSink {
  /// Creates a logging sink.
  LoggingInputSink();

  int _count = 0;

  /// Number of events sent so far.
  int get count => _count;

  @override
  void send(InputEvent event) {
    _count++;
    developer.log(jsonEncode(event.toJson()), name: 'input');
  }

  @override
  void sendAll(Iterable<InputEvent> events) {
    for (final e in events) {
      send(e);
    }
  }
}

/// Provides the active [InputSink]. Overridden in Phase 5 with the data-channel
/// sink, and in tests with a recording fake.
final inputSinkProvider = Provider<InputSink>((ref) => LoggingInputSink());
