import 'package:flutter/foundation.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/storage/secure_storage.dart';
import '../../../core/util/keys.dart';
import '../data/device_api.dart';
import '../domain/device.dart';

/// Resolves and persists this phone's identity as a registered device.
///
/// On first use it generates a device key, registers with the device service,
/// and stores the server-assigned device id. Pairing uses that id as the
/// `mobile_device_id`. The uploaded public key is a placeholder until the real
/// X25519 identity lands with end-to-end encryption; what pairing needs today is
/// a stable, server-known device row.
class DeviceIdentity {
  /// Creates the identity service.
  DeviceIdentity(this._api, this._store);

  final DeviceApi _api;
  final SecureStore _store;

  /// Return this device's server-assigned id, registering it on first use.
  Future<String> ensureRegistered() async {
    final existing = await _store.read(StorageKeys.mobileDeviceId);
    if (existing != null && existing.isNotEmpty) return existing;

    final publicKey = await _ensurePublicKey();
    final device = await _api.register(
      DeviceRegistration(
        kind: DeviceKind.mobile,
        platform: _platform(),
        name: _deviceName(),
        publicKey: publicKey,
      ),
    );
    await _store.write(StorageKeys.mobileDeviceId, device.id);
    return device.id;
  }

  Future<String> _ensurePublicKey() async {
    final existing = await _store.read(StorageKeys.mobileDevicePublicKey);
    if (existing != null && existing.isNotEmpty) return existing;
    final key = generateRandomKeyBase64();
    await _store.write(StorageKeys.mobileDevicePublicKey, key);
    return key;
  }

  String _platform() {
    switch (defaultTargetPlatform) {
      case TargetPlatform.iOS:
        return 'ios';
      case TargetPlatform.android:
        return 'android';
      default:
        // The app ships on iOS/Android; default keeps a valid enum value when
        // running on a desktop host (e.g. tests, emulators).
        return 'android';
    }
  }

  String _deviceName() {
    switch (defaultTargetPlatform) {
      case TargetPlatform.iOS:
        return 'iPhone';
      default:
        return 'Android device';
    }
  }
}

/// Provides the [DeviceIdentity] service.
final deviceIdentityProvider = Provider<DeviceIdentity>((ref) {
  return DeviceIdentity(
    ref.watch(deviceApiProvider),
    ref.watch(secureStoreProvider),
  );
});
