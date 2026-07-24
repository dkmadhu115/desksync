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

/// An [InputSink] whose destination can be swapped at runtime.
///
/// The viewer's input pipeline (gestures/keyboard) always dispatches through a
/// single, stable sink. The live WebRTC data-channel sink only exists after a
/// session connects, so this sink forwards to whatever [target] is currently
/// set and, until then (or after teardown), falls back to a [fallback]
/// (logging) sink and counts the frames it drops from the wire. This keeps
/// [InputController] and the widgets decoupled from the WebRTC lifecycle.
class SwitchableInputSink implements InputSink {
  /// Creates a switchable sink with an optional [fallback] used when no
  /// [target] is attached (defaults to a [LoggingInputSink]).
  SwitchableInputSink({InputSink? fallback})
      : _fallback = fallback ?? LoggingInputSink();

  final InputSink _fallback;
  InputSink? _target;

  /// Number of events that were sent while no live target was attached.
  int droppedFromWire = 0;

  /// Whether a live target is currently attached.
  bool get hasTarget => _target != null;

  /// Attach the live destination (e.g. the session's data-channel sink).
  void attach(InputSink target) => _target = target;

  /// Detach the live destination; subsequent events go to the fallback.
  void detach() => _target = null;

  @override
  void send(InputEvent event) {
    final target = _target;
    if (target != null) {
      target.send(event);
    } else {
      droppedFromWire++;
      _fallback.send(event);
    }
  }

  @override
  void sendAll(Iterable<InputEvent> events) {
    for (final e in events) {
      send(e);
    }
  }
}

/// Provides the process-wide [SwitchableInputSink]. The viewer attaches the
/// live data-channel sink to it on connect and detaches on teardown.
final switchableInputSinkProvider =
    Provider<SwitchableInputSink>((ref) => SwitchableInputSink());

/// Provides the active [InputSink] used by [InputController]. Backed by the
/// [switchableInputSinkProvider] so input transparently flows to the live
/// WebRTC channel when connected; overridden in tests with a recording fake.
final inputSinkProvider =
    Provider<InputSink>((ref) => ref.watch(switchableInputSinkProvider));
