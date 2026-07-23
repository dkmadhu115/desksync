/// The signaling wire protocol, mirroring the backend (`services/signaling`)
/// and the Rust agent (`desksync-transport`). Envelopes carry a monotonic nonce
/// and timestamp for replay protection; the payload is a tagged union keyed by
/// `kind`. The signaling server relays offer/answer/ice_candidate verbatim and
/// injects peer_joined/peer_left presence control messages.
library;

/// Payload discriminator values.
class SignalKind {
  SignalKind._();

  /// SDP offer.
  static const offer = 'offer';

  /// SDP answer.
  static const answer = 'answer';

  /// A trickled ICE candidate.
  static const iceCandidate = 'ice_candidate';

  /// Keep-alive.
  static const heartbeat = 'heartbeat';

  /// Teardown request.
  static const bye = 'bye';

  /// Server control: other peer joined.
  static const peerJoined = 'peer_joined';

  /// Server control: other peer left.
  static const peerLeft = 'peer_left';
}

/// A decoded signaling payload.
sealed class SignalPayload {
  const SignalPayload();

  /// The `kind` discriminator.
  String get kind;

  /// The JSON object for this payload (including `kind`).
  Map<String, dynamic> toJson();

  /// Parse a payload object; unknown kinds become [UnknownPayload] so the
  /// client tolerates forward-compatible additions.
  factory SignalPayload.fromJson(Map<String, dynamic> json) {
    switch (json['kind']) {
      case SignalKind.offer:
        return OfferPayload(sdp: json['sdp'] as String? ?? '');
      case SignalKind.answer:
        return AnswerPayload(sdp: json['sdp'] as String? ?? '');
      case SignalKind.iceCandidate:
        return IceCandidatePayload(
          candidate: json['candidate'] as String? ?? '',
          sdpMLineIndex: (json['sdp_m_line_index'] as num?)?.toInt() ?? 0,
        );
      case SignalKind.heartbeat:
        return const HeartbeatPayload();
      case SignalKind.bye:
        return const ByePayload();
      case SignalKind.peerJoined:
        return PeerJoinedPayload(role: json['role'] as String? ?? '');
      case SignalKind.peerLeft:
        return PeerLeftPayload(role: json['role'] as String? ?? '');
      default:
        return UnknownPayload(json['kind']?.toString() ?? '');
    }
  }
}

/// SDP offer.
class OfferPayload extends SignalPayload {
  /// Creates an offer.
  const OfferPayload({required this.sdp});

  /// The SDP.
  final String sdp;
  @override
  String get kind => SignalKind.offer;
  @override
  Map<String, dynamic> toJson() => {'kind': kind, 'sdp': sdp};
}

/// SDP answer.
class AnswerPayload extends SignalPayload {
  /// Creates an answer.
  const AnswerPayload({required this.sdp});

  /// The SDP.
  final String sdp;
  @override
  String get kind => SignalKind.answer;
  @override
  Map<String, dynamic> toJson() => {'kind': kind, 'sdp': sdp};
}

/// A trickled ICE candidate.
class IceCandidatePayload extends SignalPayload {
  /// Creates a candidate payload.
  const IceCandidatePayload({
    required this.candidate,
    required this.sdpMLineIndex,
  });

  /// The candidate line.
  final String candidate;

  /// The media line index.
  final int sdpMLineIndex;
  @override
  String get kind => SignalKind.iceCandidate;
  @override
  Map<String, dynamic> toJson() =>
      {'kind': kind, 'candidate': candidate, 'sdp_m_line_index': sdpMLineIndex};
}

/// Keep-alive heartbeat.
class HeartbeatPayload extends SignalPayload {
  /// Creates a heartbeat.
  const HeartbeatPayload();
  @override
  String get kind => SignalKind.heartbeat;
  @override
  Map<String, dynamic> toJson() => {'kind': kind};
}

/// Teardown request.
class ByePayload extends SignalPayload {
  /// Creates a bye.
  const ByePayload();
  @override
  String get kind => SignalKind.bye;
  @override
  Map<String, dynamic> toJson() => {'kind': kind};
}

/// Server control: the other peer joined.
class PeerJoinedPayload extends SignalPayload {
  /// Creates a peer-joined control.
  const PeerJoinedPayload({required this.role});

  /// The role that joined.
  final String role;
  @override
  String get kind => SignalKind.peerJoined;
  @override
  Map<String, dynamic> toJson() => {'kind': kind, 'role': role};
}

/// Server control: the other peer left.
class PeerLeftPayload extends SignalPayload {
  /// Creates a peer-left control.
  const PeerLeftPayload({required this.role});

  /// The role that left.
  final String role;
  @override
  String get kind => SignalKind.peerLeft;
  @override
  Map<String, dynamic> toJson() => {'kind': kind, 'role': role};
}

/// An unrecognized payload kind.
class UnknownPayload extends SignalPayload {
  /// Creates an unknown payload wrapper.
  const UnknownPayload(this._kind);
  final String _kind;
  @override
  String get kind => _kind;
  @override
  Map<String, dynamic> toJson() => {'kind': _kind};
}

/// The signaling envelope.
class SignalEnvelope {
  /// Creates an envelope.
  const SignalEnvelope({
    required this.sessionId,
    required this.nonce,
    required this.payload,
    this.version = 1,
    this.tsMs = 0,
  });

  /// Protocol version.
  final int version;

  /// Monotonic per-connection nonce.
  final int nonce;

  /// Creation time (unix ms).
  final int tsMs;

  /// Session id.
  final String sessionId;

  /// The payload.
  final SignalPayload payload;

  /// The wire representation.
  Map<String, dynamic> toJson() => {
        'v': version,
        'nonce': nonce,
        'ts_ms': tsMs,
        'session_id': sessionId,
        'payload': payload.toJson(),
      };

  /// Parse from wire JSON.
  factory SignalEnvelope.fromJson(Map<String, dynamic> json) {
    return SignalEnvelope(
      version: (json['v'] as num?)?.toInt() ?? 1,
      nonce: (json['nonce'] as num?)?.toInt() ?? 0,
      tsMs: (json['ts_ms'] as num?)?.toInt() ?? 0,
      sessionId: json['session_id'] as String? ?? '',
      payload: SignalPayload.fromJson(
        (json['payload'] as Map<String, dynamic>?) ?? const {'kind': ''},
      ),
    );
  }
}
