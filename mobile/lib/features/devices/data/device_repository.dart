import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../domain/device.dart';
import 'device_api.dart';

/// Repository over the device service. Currently a thin pass-through, it is the
/// seam where local caching (Hive) can be added without touching the UI.
class DeviceRepository {
  /// Creates the repository.
  DeviceRepository(this._api);

  final DeviceApi _api;

  /// Fetch the caller's devices.
  Future<List<Device>> listDevices() => _api.list();

  /// Revoke and remove a device.
  Future<void> deleteDevice(String deviceId) => _api.delete(deviceId);
}

/// Provides the [DeviceRepository].
final deviceRepositoryProvider = Provider<DeviceRepository>((ref) {
  return DeviceRepository(ref.watch(deviceApiProvider));
});
