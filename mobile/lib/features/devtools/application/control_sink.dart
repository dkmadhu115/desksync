import 'dart:convert';
import 'dart:developer' as developer;

import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Destination for control-plane JSON frames (dev actions) sent to the agent
/// over the WebRTC "control" data channel.
///
/// Kept separate from the input channel so latency-sensitive input is never
/// blocked behind larger control payloads, and so each channel can evolve
/// independently. Mirrors the input pipeline's [SwitchableInputSink] design.
abstract interface class ControlSink {
  /// Send a single JSON frame.
  void send(String jsonFrame);
}

/// A control sink whose destination can be swapped at runtime. The Quick Launch
/// UI always dispatches through this stable sink; the live data-channel target
/// is attached on connect and detached on teardown. Frames sent with no target
/// are counted and logged (dropped from the wire).
class SwitchableControlSink implements ControlSink {
  /// Creates the sink.
  SwitchableControlSink();

  ControlSink? _target;

  /// Number of frames sent while no live target was attached.
  int droppedFromWire = 0;

  /// Whether a live target is attached.
  bool get hasTarget => _target != null;

  /// Attach the live destination.
  void attach(ControlSink target) => _target = target;

  /// Detach the live destination.
  void detach() => _target = null;

  @override
  void send(String jsonFrame) {
    final target = _target;
    if (target != null) {
      target.send(jsonFrame);
    } else {
      droppedFromWire++;
      developer.log(jsonFrame, name: 'devtools.control.dropped');
    }
  }
}

/// A control sink backed by a plain send callback (the WebRTC data channel).
class CallbackControlSink implements ControlSink {
  /// Creates the sink over [_send].
  CallbackControlSink(this._send);

  final void Function(String jsonFrame) _send;

  @override
  void send(String jsonFrame) => _send(jsonFrame);
}

/// Encodes [json] as a compact frame string.
String encodeControlFrame(Map<String, dynamic> json) => jsonEncode(json);

/// Provides the process-wide [SwitchableControlSink].
final switchableControlSinkProvider =
    Provider<SwitchableControlSink>((ref) => SwitchableControlSink());
