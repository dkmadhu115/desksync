/// Lifecycle status of a pairing.
enum PairingStatus {
  /// Awaiting confirmation from the mobile device.
  pending,

  /// Confirmed and usable.
  active,

  /// Revoked; no longer usable.
  revoked,

  /// Unknown/forward-compatible value.
  unknown,
}

/// The challenge returned when pairing is initiated, mirroring the
/// `PairingChallenge` schema. The desktop displays [qrPayload] (as a QR) and
/// [manualCode] (for manual entry).
class PairingChallenge {
  /// Creates a challenge.
  const PairingChallenge({
    required this.pairingId,
    required this.qrPayload,
    required this.manualCode,
    this.expiresAt,
  });

  /// Server-assigned pairing id (UUID).
  final String pairingId;

  /// Opaque string to encode as a QR code.
  final String qrPayload;

  /// 8-digit human-enterable code.
  final String manualCode;

  /// When the challenge expires.
  final DateTime? expiresAt;

  /// Parse from the backend JSON body.
  factory PairingChallenge.fromJson(Map<String, dynamic> json) {
    return PairingChallenge(
      pairingId: json['pairing_id'] as String,
      qrPayload: (json['qr_payload'] as String?) ?? '',
      manualCode: (json['manual_code'] as String?) ?? '',
      expiresAt: _parseDate(json['expires_at']),
    );
  }
}

/// A confirmed (or pending/revoked) pairing between a mobile and a desktop,
/// mirroring the `Pairing` schema.
class Pairing {
  /// Creates a pairing.
  const Pairing({
    required this.id,
    required this.mobileDeviceId,
    required this.desktopDeviceId,
    required this.status,
    required this.trusted,
    this.createdAt,
  });

  /// Pairing id (UUID).
  final String id;

  /// The mobile device in the pairing.
  final String mobileDeviceId;

  /// The desktop device in the pairing.
  final String desktopDeviceId;

  /// Current pairing status.
  final PairingStatus status;

  /// Whether the pairing has been marked trusted.
  final bool trusted;

  /// When the pairing was created.
  final DateTime? createdAt;

  /// Parse from the backend JSON body.
  factory Pairing.fromJson(Map<String, dynamic> json) {
    return Pairing(
      id: json['id'] as String,
      mobileDeviceId: (json['mobile_device_id'] as String?) ?? '',
      desktopDeviceId: (json['desktop_device_id'] as String?) ?? '',
      status: _statusFrom(json['status'] as String?),
      trusted: (json['trusted'] as bool?) ?? false,
      createdAt: _parseDate(json['created_at']),
    );
  }

  static PairingStatus _statusFrom(String? value) {
    switch (value) {
      case 'pending':
        return PairingStatus.pending;
      case 'active':
        return PairingStatus.active;
      case 'revoked':
        return PairingStatus.revoked;
      default:
        return PairingStatus.unknown;
    }
  }
}

DateTime? _parseDate(dynamic value) {
  if (value is String && value.isNotEmpty) return DateTime.tryParse(value);
  return null;
}

/// Select the active pairing for [desktopDeviceId] from [pairings], or null.
///
/// A remote-control session can only be created over an `active` pairing; this
/// filters out pending/revoked ones. If several match (e.g. re-paired), the
/// most recently created is preferred.
Pairing? selectActivePairing(
  Iterable<Pairing> pairings,
  String desktopDeviceId,
) {
  final matches = pairings
      .where((p) =>
          p.desktopDeviceId == desktopDeviceId &&
          p.status == PairingStatus.active)
      .toList()
    ..sort((a, b) {
      final at = a.createdAt;
      final bt = b.createdAt;
      if (at == null && bt == null) return 0;
      if (at == null) return 1;
      if (bt == null) return -1;
      return bt.compareTo(at);
    });
  return matches.isEmpty ? null : matches.first;
}
