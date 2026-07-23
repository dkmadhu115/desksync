/// Client models for the session service, mirroring the `Session`,
/// `IceServer`, and `SessionCreated` schemas in the OpenAPI contract. Creating
/// a session yields the signaling URL + ticket and the ICE servers the client
/// needs to establish the WebRTC connection.
library;

/// Lifecycle status of a session.
enum SessionStatus {
  /// Being set up.
  initiating,

  /// Peers negotiating the connection.
  connecting,

  /// Media/data flowing.
  active,

  /// Ended normally.
  ended,

  /// Ended abnormally.
  failed,

  /// Unrecognized value (forward-compatible).
  unknown,
}

SessionStatus _statusFrom(String? s) {
  switch (s) {
    case 'initiating':
      return SessionStatus.initiating;
    case 'connecting':
      return SessionStatus.connecting;
    case 'active':
      return SessionStatus.active;
    case 'ended':
      return SessionStatus.ended;
    case 'failed':
      return SessionStatus.failed;
    default:
      return SessionStatus.unknown;
  }
}

/// A remote-control session.
class Session {
  /// Creates a session.
  const Session({
    required this.id,
    required this.pairingId,
    required this.status,
    this.connectionType,
    this.startedAt,
    this.endedAt,
  });

  /// Session id.
  final String id;

  /// The pairing this session belongs to.
  final String pairingId;

  /// Lifecycle status.
  final SessionStatus status;

  /// How media flowed once known (`p2p` or `relay`).
  final String? connectionType;

  /// When the session started.
  final DateTime? startedAt;

  /// When the session ended, if it has.
  final DateTime? endedAt;

  /// Parse from backend JSON.
  factory Session.fromJson(Map<String, dynamic> json) {
    DateTime? parse(Object? v) =>
        v is String ? DateTime.tryParse(v)?.toLocal() : null;
    return Session(
      id: json['id'] as String,
      pairingId: json['pairing_id'] as String,
      status: _statusFrom(json['status'] as String?),
      connectionType: json['connection_type'] as String?,
      startedAt: parse(json['started_at']),
      endedAt: parse(json['ended_at']),
    );
  }
}

/// A single ICE server entry (STUN or TURN).
class IceServer {
  /// Creates an ICE server entry.
  const IceServer({required this.urls, this.username, this.credential});

  /// One or more server URLs.
  final List<String> urls;

  /// TURN username (absent for STUN).
  final String? username;

  /// TURN credential (absent for STUN).
  final String? credential;

  /// Parse from backend JSON.
  factory IceServer.fromJson(Map<String, dynamic> json) {
    final rawUrls = json['urls'];
    final urls = <String>[
      if (rawUrls is List) ...rawUrls.whereType<String>(),
      if (rawUrls is String) rawUrls,
    ];
    return IceServer(
      urls: urls,
      username: json['username'] as String?,
      credential: json['credential'] as String?,
    );
  }

  /// The RTCConfiguration entry shape expected by flutter_webrtc.
  Map<String, dynamic> toRtcConfig() => {
        'urls': urls,
        if (username != null) 'username': username,
        if (credential != null) 'credential': credential,
      };
}

/// The result of creating a session.
class SessionCreated {
  /// Creates the result.
  const SessionCreated({
    required this.session,
    required this.signalingUrl,
    required this.signalingTicket,
    required this.iceServers,
  });

  /// The created session.
  final Session session;

  /// Secure WebSocket URL for the signaling channel.
  final String signalingUrl;

  /// Short-lived token authorizing the WebSocket upgrade.
  final String signalingTicket;

  /// ICE servers for the peer connection.
  final List<IceServer> iceServers;

  /// Parse from backend JSON.
  factory SessionCreated.fromJson(Map<String, dynamic> json) {
    final rawIce = json['ice_servers'];
    final ice = <IceServer>[
      if (rawIce is List)
        ...rawIce
            .whereType<Map<String, dynamic>>()
            .map(IceServer.fromJson),
    ];
    return SessionCreated(
      session: Session.fromJson(json['session'] as Map<String, dynamic>),
      signalingUrl: json['signaling_url'] as String,
      signalingTicket: json['signaling_ticket'] as String,
      iceServers: ice,
    );
  }

  /// The full RTCConfiguration for flutter_webrtc's `createPeerConnection`.
  Map<String, dynamic> toRtcConfiguration() => {
        'iceServers': iceServers.map((s) => s.toRtcConfig()).toList(),
        'sdpSemantics': 'unified-plan',
      };
}
