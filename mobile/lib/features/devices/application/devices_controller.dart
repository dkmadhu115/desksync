import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../data/device_repository.dart';
import '../domain/device.dart';

/// Loads and manages the user's device list. Exposes an [AsyncValue] so the UI
/// can render loading / error / data uniformly, with pull-to-refresh and
/// optimistic removal.
class DevicesController extends AsyncNotifier<List<Device>> {
  DeviceRepository get _repo => ref.read(deviceRepositoryProvider);

  @override
  Future<List<Device>> build() => _repo.listDevices();

  /// Re-fetch the device list, surfacing loading/error via [state].
  Future<void> refresh() async {
    state = const AsyncValue.loading();
    state = await AsyncValue.guard(_repo.listDevices);
  }

  /// Remove a device, updating the list optimistically and rolling back on
  /// failure.
  Future<void> remove(String deviceId) async {
    final previous = state.value ?? const <Device>[];
    state = AsyncValue.data(
      previous.where((d) => d.id != deviceId).toList(growable: false),
    );
    try {
      await _repo.deleteDevice(deviceId);
    } catch (_) {
      // Roll back to the previous list on failure.
      state = AsyncValue.data(previous);
      rethrow;
    }
  }
}

/// Provides the [DevicesController].
final devicesControllerProvider =
    AsyncNotifierProvider<DevicesController, List<Device>>(
  DevicesController.new,
);
