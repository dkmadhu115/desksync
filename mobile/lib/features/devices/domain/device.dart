/// The kind of a device.
enum DeviceKind {
  /// A controllable desktop/laptop running the agent.
  desktop,

  /// A mobile client.
  mobile,

  /// Unknown/forward-compatible value.
  unknown,
}

/// Online/offline presence of a device.
enum DeviceStatus {
  /// Connected and reachable.
  online,

  /// Not currently connected.
  offline,
}

/// A device belonging to the signed-in user, mirroring the `Device` schema in
/// the OpenAPI contract.
class Device {
  /// Creates a device.
  const Device({
    required this.id,
    required this.kind,
    required this.platform,
    required this.name,
    required this.status,
    this.lastSeenAt,
    this.createdAt,
  });

  /// Server-assigned device id (UUID).
  final String id;

  /// Whether this is a desktop or mobile device.
  final DeviceKind kind;

  /// OS platform string (windows/macos/linux/android/ios).
  final String platform;

  /// Human-friendly device name.
  final String name;

  /// Current presence.
  final DeviceStatus status;

  /// When the device was last seen online, if ever.
  final DateTime? lastSeenAt;

  /// When the device was registered.
  final DateTime? createdAt;

  /// Whether this device is a desktop that is currently online (i.e. can be
  /// controlled right now).
  bool get isControllable =>
      kind == DeviceKind.desktop && status == DeviceStatus.online;

  /// Parse from the backend JSON body.
  factory Device.fromJson(Map<String, dynamic> json) {
    return Device(
      id: json['id'] as String,
      kind: _kindFrom(json['kind'] as String?),
      platform: (json['platform'] as String?) ?? 'unknown',
      name: (json['name'] as String?) ?? 'Unnamed device',
      status: (json['status'] as String?) == 'online'
          ? DeviceStatus.online
          : DeviceStatus.offline,
      lastSeenAt: _parseDate(json['last_seen_at']),
      createdAt: _parseDate(json['created_at']),
    );
  }

  static DeviceKind _kindFrom(String? value) {
    switch (value) {
      case 'desktop':
        return DeviceKind.desktop;
      case 'mobile':
        return DeviceKind.mobile;
      default:
        return DeviceKind.unknown;
    }
  }

  static DateTime? _parseDate(dynamic value) {
    if (value is String && value.isNotEmpty) {
      return DateTime.tryParse(value);
    }
    return null;
  }
}
