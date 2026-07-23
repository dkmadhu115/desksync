import 'dart:async';
import 'dart:convert';
import 'dart:io';

import '../domain/signal_envelope.dart';

/// A WebSocket client for the signaling service. It connects with the
/// short-lived ticket, decodes incoming [SignalEnvelope]s onto a broadcast
/// [messages] stream, stamps outgoing messages with a monotonic nonce, and
/// keeps the connection alive with periodic heartbeats. It deals only in
/// signaling — media flows over the WebRTC peer connection, not here.
class SignalingClient {
  /// Creates a client bound to a session.
  SignalingClient({required this.sessionId, Duration? heartbeatInterval})
      : _heartbeatInterval = heartbeatInterval ?? const Duration(seconds: 25);

  /// The session this client signals for.
  final String sessionId;

  final Duration _heartbeatInterval;
  final StreamController<SignalEnvelope> _controller =
      StreamController<SignalEnvelope>.broadcast();

  WebSocket? _socket;
  int _nonce = 0;
  Timer? _heartbeat;

  /// Incoming decoded envelopes.
  Stream<SignalEnvelope> get messages => _controller.stream;

  /// Whether the socket is currently open.
  bool get isConnected => _socket != null;

  /// Connect to [url] (the session's `signaling_url`) with the [ticket] and
  /// [role] (`controller` or `agent`).
  Future<void> connect({
    required String url,
    required String ticket,
    required String role,
  }) async {
    final sep = url.contains('?') ? '&' : '?';
    final full = '$url${sep}ticket='
        '${Uri.encodeQueryComponent(ticket)}'
        '&session=${Uri.encodeQueryComponent(sessionId)}'
        '&role=${Uri.encodeQueryComponent(role)}';

    final socket = await WebSocket.connect(full);
    _socket = socket;
    socket.listen(
      _onData,
      onDone: _onClosed,
      onError: (Object _) => _onClosed(),
      cancelOnError: true,
    );
    _startHeartbeat();
  }

  /// Send a payload; a no-op when disconnected.
  void send(SignalPayload payload) {
    final socket = _socket;
    if (socket == null) return;
    _nonce++;
    final env = SignalEnvelope(
      sessionId: sessionId,
      nonce: _nonce,
      tsMs: DateTime.now().millisecondsSinceEpoch,
      payload: payload,
    );
    socket.add(jsonEncode(env.toJson()));
  }

  /// Close the connection and release resources.
  Future<void> close() async {
    _heartbeat?.cancel();
    _heartbeat = null;
    final socket = _socket;
    _socket = null;
    await socket?.close();
    if (!_controller.isClosed) {
      await _controller.close();
    }
  }

  void _onData(dynamic data) {
    if (data is! String) return;
    try {
      final json = jsonDecode(data) as Map<String, dynamic>;
      final env = SignalEnvelope.fromJson(json);
      if (!_controller.isClosed) _controller.add(env);
    } catch (_) {
      // Ignore malformed frames rather than tearing down the session.
    }
  }

  void _onClosed() {
    _heartbeat?.cancel();
    _heartbeat = null;
    _socket = null;
    if (!_controller.isClosed) _controller.close();
  }

  void _startHeartbeat() {
    _heartbeat?.cancel();
    _heartbeat = Timer.periodic(_heartbeatInterval, (_) {
      send(const HeartbeatPayload());
    });
  }
}
