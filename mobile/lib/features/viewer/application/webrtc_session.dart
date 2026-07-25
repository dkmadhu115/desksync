import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/foundation.dart';
import 'package:flutter/scheduler.dart';
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

  /// Renders the remote desktop's video track once it arrives (reserved for a
  /// future hardware video track; the current agent streams JPEG frames over
  /// the [videoFrame] data channel instead).
  final RTCVideoRenderer remoteRenderer = RTCVideoRenderer();

  /// The latest **decoded** screen frame received from the desktop over the
  /// `frames` data channel. The UI paints this with `RawImage`.
  ///
  /// We decode the incoming JPEG bytes into a [ui.Image] ourselves (rather than
  /// feeding raw bytes to `Image.memory`) for two critical reasons:
  ///  * `Image.memory` routes every unique byte buffer through Flutter's
  ///    `ImageCache`; at ~30fps of full-screen JPEGs each ~7.5MB decoded bitmap
  ///    piles up in the cache and exhausts memory within a couple of seconds,
  ///    which crashes the app (the "connects then closes" symptom).
  ///  * Manual decoding lets us hold exactly one frame, dispose the previous
  ///    one, and drop frames while a decode is in flight — bounding memory.
  final ValueNotifier<ui.Image?> videoFrame = ValueNotifier(null);

  // Frame decode pipeline. `_pendingFrame` always holds the newest undecoded
  // frame; older ones are dropped so a slow decode can never build a backlog.
  Uint8List? _pendingFrame;
  bool _decoding = false;

  // Frame reassembly. The agent splits each JPEG frame into 16 KiB chunks
  // (each prefixed with an 8-byte header) because a whole frame exceeds the
  // data channel's max message size. The `frames` channel is reliable+ordered,
  // so chunks arrive in send order; we accumulate them until a frame is complete
  // and drop a partial frame if a newer one starts (newest wins for live video).
  static const int _frameChunkHeaderLen = 8;
  int? _asmFrameId;
  int _asmExpectedChunks = 0;
  final List<Uint8List> _asmChunks = [];

  /// Observable connection phase for the UI.
  final ValueNotifier<WebRtcPhase> phase = ValueNotifier(WebRtcPhase.idle);

  RTCPeerConnection? _pc;
  RTCDataChannel? _inputChannel;
  RTCDataChannel? _controlChannel;
  RTCDataChannel? _framesChannel;
  StreamSubscription<SignalEnvelope>? _sub;
  DataChannelInputSink? _inputSink;
  ControlSink? _controlSink;

  // Negotiation guards. WebRTC's native peer connection crashes hard on illegal
  // transitions (e.g. addCandidate before the remote description is applied, a
  // second createOffer, or applying an answer twice). Signaling arrives as an
  // async stream whose callbacks are NOT serialized, so we (1) chain handling so
  // one message finishes before the next starts, (2) offer/apply-answer at most
  // once, and (3) buffer remote ICE candidates until the answer is applied.
  Future<void> _signalChain = Future<void>.value();
  bool _offerSent = false;
  bool _remoteDescriptionSet = false;
  bool _closed = false;
  final List<RTCIceCandidate> _pendingRemoteIce = [];

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

    // Bidirectional channel the desktop uses to push encoded screen frames. We
    // create it (as the offerer) and only listen; the agent sends JPEG bytes.
    final frames = await pc.createDataChannel(
      'frames',
      RTCDataChannelInit()..ordered = true,
    );
    _framesChannel = frames;
    frames.onMessage = (message) {
      if (message.isBinary) {
        _onFrameChunk(message.binary);
      }
    };

    pc.onIceCandidate = (candidate) {
      final line = candidate.candidate;
      if (line == null || line.isEmpty) return;
      _signaling.send(IceCandidatePayload(
        candidate: line,
        sdpMLineIndex: candidate.sdpMLineIndex ?? 0,
        sdpMid: candidate.sdpMid,
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

  // Serialize signal handling: each message fully completes before the next is
  // processed. Without this, an ICE candidate could reach the native
  // addCandidate before setRemoteDescription(answer) resolves, crashing the app.
  Future<void> _onSignal(SignalEnvelope env) {
    _signalChain = _signalChain.then((_) => _handleSignal(env));
    return _signalChain;
  }

  Future<void> _handleSignal(SignalEnvelope env) async {
    final pc = _pc;
    if (pc == null || _closed) return;
    try {
      switch (env.payload) {
        case PeerJoinedPayload():
          // Offer exactly once, even if presence is announced repeatedly (e.g.
          // the agent reconnects to signaling).
          if (_offerSent) return;
          _offerSent = true;
          await _createAndSendOffer(pc);
        case AnswerPayload(:final sdp):
          // Apply the answer once; a duplicate would be an illegal transition.
          if (_remoteDescriptionSet) return;
          await pc.setRemoteDescription(RTCSessionDescription(sdp, 'answer'));
          _remoteDescriptionSet = true;
          // Flush any candidates that arrived before the answer.
          for (final c in _pendingRemoteIce) {
            await pc.addCandidate(c);
          }
          _pendingRemoteIce.clear();
        case IceCandidatePayload(
            :final candidate,
            :final sdpMLineIndex,
            :final sdpMid,
          ):
          // NEVER hand a null sdpMid to the native peer connection: the Android
          // `org.webrtc` build NPEs in JniHelper.getStringBytes and hard-crashes
          // the app. When the peer didn't send a mid, fall back to the m-line
          // index as a string, which is the default mid libwebrtc/webrtc-rs
          // assign ("0", "1", ...), so it still matches the correct section.
          final mid = (sdpMid != null && sdpMid.isNotEmpty)
              ? sdpMid
              : '$sdpMLineIndex';
          final c = RTCIceCandidate(candidate, mid, sdpMLineIndex);
          if (_remoteDescriptionSet) {
            await pc.addCandidate(c);
          } else {
            // Buffer until the remote description exists to avoid a native crash.
            _pendingRemoteIce.add(c);
          }
        case PeerLeftPayload() || ByePayload():
          await close();
        // The controller is the offerer; it ignores offers and control frames.
        case OfferPayload() ||
              HeartbeatPayload() ||
              UnknownPayload():
          break;
      }
    } catch (e, st) {
      // Never let a signaling hiccup take down the app; surface as a failed
      // connection instead.
      debugPrint('WebRtcSession: error handling ${env.payload}: $e\n$st');
      phase.value = WebRtcPhase.failed;
    }
  }

  Future<void> _createAndSendOffer(RTCPeerConnection pc) async {
    final offer = await pc.createOffer();
    await pc.setLocalDescription(offer);
    _signaling.send(OfferPayload(sdp: offer.sdp ?? ''));
  }

  /// Reassemble one chunk of a screen frame. Each chunk is
  /// `[frame_id:u32][chunk_index:u16][chunk_count:u16][payload…]` (little-endian
  /// header). A completed frame's JPEG bytes are handed to [_onFrameBytes].
  void _onFrameChunk(Uint8List bytes) {
    if (bytes.length < _frameChunkHeaderLen) return;
    final header = ByteData.sublistView(bytes, 0, _frameChunkHeaderLen);
    final frameId = header.getUint32(0, Endian.little);
    final chunkIndex = header.getUint16(4, Endian.little);
    final chunkCount = header.getUint16(6, Endian.little);
    if (chunkCount == 0) return;
    // Copy the payload out of the incoming buffer: the plugin may reuse the
    // underlying byte buffer for the next message, which would corrupt chunks we
    // hold across messages during reassembly.
    final payload = Uint8List.fromList(
      Uint8List.sublistView(bytes, _frameChunkHeaderLen),
    );

    if (chunkIndex == 0) {
      // Start of a new frame: reset the assembly buffer.
      _asmFrameId = frameId;
      _asmExpectedChunks = chunkCount;
      _asmChunks
        ..clear()
        ..add(payload);
    } else {
      // Continuation: it must belong to the frame we're assembling and arrive in
      // order. Otherwise we missed a chunk — drop the partial frame and wait for
      // the next frame's first chunk.
      if (_asmFrameId != frameId || _asmChunks.length != chunkIndex) {
        _asmFrameId = null;
        _asmChunks.clear();
        return;
      }
      _asmChunks.add(payload);
    }

    if (_asmChunks.length == _asmExpectedChunks) {
      final total = _asmChunks.fold<int>(0, (n, c) => n + c.length);
      final full = Uint8List(total);
      var offset = 0;
      for (final c in _asmChunks) {
        full.setRange(offset, offset + c.length, c);
        offset += c.length;
      }
      _asmFrameId = null;
      _asmChunks.clear();
      _onFrameBytes(full);
    }
  }

  /// Queue a raw JPEG frame for decoding. Only the most recent frame is kept;
  /// if a decode is already running, this just replaces the pending frame so we
  /// never accumulate a backlog (and never exhaust memory) under load.
  void _onFrameBytes(Uint8List bytes) {
    _pendingFrame = bytes;
    if (_decoding) return;
    unawaited(_decodeLoop());
  }

  Future<void> _decodeLoop() async {
    _decoding = true;
    try {
      while (!_closed && _pendingFrame != null) {
        final bytes = _pendingFrame!;
        _pendingFrame = null;
        ui.Image? decoded;
        try {
          final codec = await ui.instantiateImageCodec(bytes);
          final frame = await codec.getNextFrame();
          codec.dispose();
          decoded = frame.image;
        } catch (e) {
          // A partial/corrupt frame must never take down the session; keep the
          // last good image on screen and move on to the next frame.
          debugPrint('WebRtcSession: frame decode failed: $e');
          continue;
        }
        if (_closed) {
          decoded.dispose();
          return;
        }
        final previous = videoFrame.value;
        videoFrame.value = decoded;
        // Dispose the previous frame only after the current frame has been
        // painted, so we never free an image the renderer is still using.
        if (previous != null) {
          SchedulerBinding.instance
              .addPostFrameCallback((_) => previous.dispose());
        }
        if (phase.value == WebRtcPhase.connecting) {
          phase.value = WebRtcPhase.connected;
        }
      }
    } finally {
      _decoding = false;
    }
  }

  /// Tear down the session and release all resources.
  Future<void> close() async {
    if (_closed) return;
    _closed = true;
    _pendingRemoteIce.clear();
    _pendingFrame = null;
    _asmChunks.clear();
    _asmFrameId = null;
    phase.value = WebRtcPhase.closed;
    await _sub?.cancel();
    _sub = null;
    if (_signaling.isConnected) {
      _signaling.send(const ByePayload());
    }
    await _signaling.close();
    await _inputChannel?.close();
    await _controlChannel?.close();
    await _framesChannel?.close();
    await _pc?.close();
    await remoteRenderer.dispose();
    final lastFrame = videoFrame.value;
    videoFrame.value = null;
    lastFrame?.dispose();
  }
}

/// Provides a factory for building a [WebRtcSession] from a created session.
final webRtcSessionFactoryProvider =
    Provider<WebRtcSession Function(SessionCreated)>((ref) {
  return (created) => WebRtcSession(created: created);
});
