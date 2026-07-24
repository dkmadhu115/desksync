import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';

import '../../devtools/application/control_sink.dart';
import '../../session/domain/session.dart';
import '../../signaling/data/signaling_client.dart';
import '../../signaling/domain/signal_envelope.dart';
import 'data_channel_input_sink.dart';
import 'input_sink.dart';

/// High-level connection phase surfaced to the UI.
enum WebRtcPhase {
  /// Not started.
  idle,

  /// Establishing signaling + peer connection.
  connecting,

  /// Media/data path established.
  connected,

  /// Connection failed.
  failed,

  /// Connection closed/torn down.
  closed,
}

/// Drives one remote-control session on the controller (mobile) side: it is the
/// WebRTC **offerer**. It creates the peer connection from the session's ICE
/// config, opens the input data channel, waits for the agent to join over
/// signaling, then exchanges the SDP offer/answer and ICE candidates. Incoming
/// video is surfaced through [remoteRenderer]; outgoing input flows through
/// [inputSink] (a [DataChannelInputSink]).
///
/// The pure collaborators (session/signaling models, the input sink, adaptive
/// bitrate) are unit-tested; this orchestrator wires them to `flutter_webrtc`,
/// whose peer connection requires a real device and is exercised in end-to-end
/// testing rather than CI.
class WebRtcSession {
  /// Creates a session driver. A custom [signaling] client can be injected for
  /// testing; otherwise one is created for the session.
  WebRtcSession({required this.created, SignalingClient? signaling})
      : _signaling =
            signaling ?? SignalingClient(sessionId: created.session.id);

  /// The session details (ids, signaling URL/ticket, ICE servers).
  final SessionCreated created;
  final SignalingClient _signaling;

  /// Renders the remote desktop's video track once it arrives.
  final RTCVideoRenderer remoteRenderer = RTCVideoRenderer();

  /// Observable connection phase for the UI.
  final ValueNotifier<WebRtcPhase> phase = ValueNotifier(WebRtcPhase.idle);

  RTCPeerConnection? _pc;
  RTCDataChannel? _inputChannel;
  RTCDataChannel? _controlChannel;
  StreamSubscription<SignalEnvelope>? _sub;
  DataChannelInputSink? _inputSink;
  ControlSink? _controlSink;

  /// The sink that forwards input events to the desktop, available once
  /// [start] has created the data channel.
  InputSink? get inputSink => _inputSink;

  /// The sink that forwards developer control actions to the desktop, available
  /// once [start] has created the control data channel.
  ControlSink? get controlSink => _controlSink;

  /// Establish the connection: set up the peer connection and data channel,
  /// connect signaling, and begin negotiation when the agent appears.
  Future<void> start() async {
    phase.value = WebRtcPhase.connecting;
    await remoteRenderer.initialize();

    final pc = await createPeerConnection(created.toRtcConfiguration());
    _pc = pc;

    // We only receive video from the desktop.
    await pc.addTransceiver(
      kind: RTCRtpMediaType.RTCRtpMediaTypeVideo,
      init: RTCRtpTransceiverInit(direction: TransceiverDirection.RecvOnly),
    );

    // Reliable, ordered channel for input events (controller -> agent).
    final channel = await pc.createDataChannel(
      'input',
      RTCDataChannelInit()..ordered = true,
    );
    _inputChannel = channel;
    _inputSink = DataChannelInputSink(
      (frame) => channel.send(RTCDataChannelMessage(frame)),
    );

    // Reliable, ordered channel for developer control actions (Quick Launch),
    // separate from input so control payloads never block latency-sensitive
    // pointer/key frames.
    final control = await pc.createDataChannel(
      'control',
      RTCDataChannelInit()..ordered = true,
    );
    _controlChannel = control;
    _controlSink = CallbackControlSink(
      (frame) => control.send(RTCDataChannelMessage(frame)),
    );

    pc.onIceCandidate = (candidate) {
      final line = candidate.candidate;
      if (line == null || line.isEmpty) return;
      _signaling.send(IceCandidatePayload(
        candidate: line,
        sdpMLineIndex: candidate.sdpMLineIndex ?? 0,
      ));
    };

    pc.onTrack = (event) {
      if (event.track.kind == 'video' && event.streams.isNotEmpty) {
        remoteRenderer.srcObject = event.streams.first;
      }
    };

    pc.onConnectionState = (state) {
      switch (state) {
        case RTCPeerConnectionState.RTCPeerConnectionStateConnected:
          phase.value = WebRtcPhase.connected;
        case RTCPeerConnectionState.RTCPeerConnectionStateFailed:
          phase.value = WebRtcPhase.failed;
        case RTCPeerConnectionState.RTCPeerConnectionStateClosed:
        case RTCPeerConnectionState.RTCPeerConnectionStateDisconnected:
          phase.value = WebRtcPhase.closed;
        default:
          break;
      }
    };

    _sub = _signaling.messages.listen(_onSignal);
    await _signaling.connect(
      url: created.signalingUrl,
      ticket: created.signalingTicket,
      role: 'controller',
    );
  }

  Future<void> _onSignal(SignalEnvelope env) async {
    final pc = _pc;
    if (pc == null) return;
    switch (env.payload) {
      case PeerJoinedPayload():
        await _createAndSendOffer(pc);
      case AnswerPayload(:final sdp):
        await pc.setRemoteDescription(RTCSessionDescription(sdp, 'answer'));
      case IceCandidatePayload(:final candidate, :final sdpMLineIndex):
        await pc.addCandidate(RTCIceCandidate(candidate, null, sdpMLineIndex));
      case PeerLeftPayload() || ByePayload():
        await close();
      // The controller is the offerer; it ignores offers and control frames.
      case OfferPayload() ||
            HeartbeatPayload() ||
            UnknownPayload():
        break;
    }
  }

  Future<void> _createAndSendOffer(RTCPeerConnection pc) async {
    final offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    _signaling.send(OfferPayload(sdp: offer.sdp ?? ''));
  }

  /// Tear down the session and release all resources.
  Future<void> close() async {
    phase.value = WebRtcPhase.closed;
    await _sub?.cancel();
    _sub = null;
    if (_signaling.isConnected) {
      _signaling.send(const ByePayload());
    }
    await _signaling.close();
    await _inputChannel?.close();
    await _controlChannel?.close();
    await _pc?.close();
    await remoteRenderer.dispose();
  }
}

/// Provides a factory for building a [WebRtcSession] from a created session.
final webRtcSessionFactoryProvider =
    Provider<WebRtcSession Function(SessionCreated)>((ref) {
  return (created) => WebRtcSession(created: created);
});
