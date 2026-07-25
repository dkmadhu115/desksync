// The named callback parameters are stored in private fields; Dart forbids
// private named parameters, so initializing formals are not applicable here.
// ignore_for_file: prefer_initializing_formals

import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

import '../../devtools/application/control_sink.dart';
import '../../pairing/data/pairing_repository.dart';
import '../../pairing/domain/pairing.dart';
import '../../session/data/session_api.dart';
import '../../session/domain/session.dart';
import 'input_sink.dart';
import 'webrtc_session.dart';

/// UI-facing lifecycle of the viewer connection.
enum ViewerPhase {
  /// Looking up the active pairing for the device.
  resolving,

  /// The device has no active pairing to connect over.
  noPairing,

  /// Establishing the session / WebRTC connection.
  connecting,

  /// Connected; remote video + input are live.
  connected,

  /// Connection failed.
  failed,

  /// Connection closed / torn down.
  closed,
}

/// Orchestrates a viewer connection for one desktop device: resolve its active
/// pairing, create a session, then drive a [WebRtcSession]. It attaches the
/// live data-channel input sink to the shared [SwitchableInputSink] so the
/// existing input pipeline flows to the desktop while connected.
///
/// The pairing/session steps are plain async calls (unit-testable via injected
/// callbacks); the WebRTC peer itself requires a real device and is exercised
/// in end-to-end testing.
class ViewerController extends ChangeNotifier {
  /// Creates a controller for [deviceId] with injectable collaborators.
  ViewerController({
    required this.deviceId,
    required Future<Pairing?> Function(String deviceId) resolvePairing,
    required Future<SessionCreated> Function(String pairingId) createSession,
    required WebRtcSession Function(SessionCreated created) sessionFactory,
    required SwitchableInputSink inputSink,
    SwitchableControlSink? controlSink,
    Future<void> Function(String sessionId)? endSession,
  })  : _resolvePairing = resolvePairing,
        _createSession = createSession,
        _sessionFactory = sessionFactory,
        _inputSink = inputSink,
        _controlSink = controlSink,
        _endSession = endSession;

  /// The desktop device being controlled.
  final String deviceId;

  final Future<Pairing?> Function(String deviceId) _resolvePairing;
  final Future<SessionCreated> Function(String pairingId) _createSession;
  final WebRtcSession Function(SessionCreated created) _sessionFactory;
  final SwitchableInputSink _inputSink;
  final SwitchableControlSink? _controlSink;
  final Future<void> Function(String sessionId)? _endSession;

  /// Current UI phase.
  ViewerPhase phase = ViewerPhase.resolving;

  /// A human-readable error, set when [phase] is [ViewerPhase.failed].
  String? errorMessage;

  WebRtcSession? _session;
  String? _sessionId;
  bool _disposed = false;

  /// The renderer showing the remote desktop, once a session exists (reserved
  /// for a future hardware video track).
  RTCVideoRenderer? get renderer => _session?.remoteRenderer;

  /// The latest decoded screen frame pushed by the desktop, once a session
  /// exists. Already decoded to a [ui.Image] so the UI can paint it directly
  /// with `RawImage` (bypassing the memory-hungry `ImageCache`).
  ValueListenable<ui.Image?>? get videoFrame => _session?.videoFrame;

  /// Resolve the pairing, create the session, and start the connection.
  Future<void> connect() async {
    _set(phase: ViewerPhase.resolving);
    try {
      final pairing = await _resolvePairing(deviceId);
      if (pairing == null) {
        _set(phase: ViewerPhase.noPairing);
        return;
      }

      final created = await _createSession(pairing.id);
      _sessionId = created.session.id;

      final session = _sessionFactory(created);
      _session = session;
      session.phase.addListener(_onSessionPhase);

      _set(phase: ViewerPhase.connecting);
      await session.start();

      // The data channels exist after start(); route input + control to them.
      final sink = session.inputSink;
      if (sink != null) _inputSink.attach(sink);
      final control = session.controlSink;
      if (control != null) _controlSink?.attach(control);
    } catch (e) {
      _fail('$e');
    }
  }

  void _onSessionPhase() {
    final s = _session;
    if (s == null) return;
    switch (s.phase.value) {
      case WebRtcPhase.connected:
        _set(phase: ViewerPhase.connected);
      case WebRtcPhase.failed:
        _fail('The connection to the desktop failed.');
      case WebRtcPhase.closed:
        _set(phase: ViewerPhase.closed);
      case WebRtcPhase.connecting:
        _set(phase: ViewerPhase.connecting);
      case WebRtcPhase.idle:
        break;
    }
  }

  /// Tear down the session, detach the input sink, and end the session on the
  /// backend (best-effort).
  Future<void> disconnect() async {
    _inputSink.detach();
    _controlSink?.detach();
    final session = _session;
    _session = null;
    session?.phase.removeListener(_onSessionPhase);
    await session?.close();

    final id = _sessionId;
    _sessionId = null;
    if (id != null && _endSession != null) {
      try {
        await _endSession(id);
      } catch (_) {
        // Ending is best-effort; the session also times out server-side.
      }
    }
  }

  void _fail(String message) {
    errorMessage = message;
    _set(phase: ViewerPhase.failed);
  }

  void _set({required ViewerPhase phase}) {
    this.phase = phase;
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    // Fire-and-forget teardown; the widget is going away.
    unawaited(disconnect());
    super.dispose();
  }
}

/// Builds a [ViewerController] wired to the app's repositories/providers for a
/// given device id.
final viewerControllerFactoryProvider =
    Provider<ViewerController Function(String deviceId)>((ref) {
  final pairingRepo = ref.watch(pairingRepositoryProvider);
  final sessionApi = ref.watch(sessionApiProvider);
  final sessionFactory = ref.watch(webRtcSessionFactoryProvider);
  final inputSink = ref.watch(switchableInputSinkProvider);
  final controlSink = ref.watch(switchableControlSinkProvider);

  return (deviceId) => ViewerController(
        deviceId: deviceId,
        resolvePairing: pairingRepo.activePairingForDevice,
        createSession: sessionApi.create,
        sessionFactory: sessionFactory,
        inputSink: inputSink,
        controlSink: controlSink,
        endSession: (id) => sessionApi.end(id),
      );
});
