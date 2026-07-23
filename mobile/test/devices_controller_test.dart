import 'package:desksync_mobile/core/network/api_exception.dart';
import 'package:desksync_mobile/features/devices/application/devices_controller.dart';
import 'package:desksync_mobile/features/devices/data/device_api.dart';
import 'package:desksync_mobile/features/devices/domain/device.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

Device _device(String id, {DeviceStatus status = DeviceStatus.offline}) => Device(
      id: id,
      kind: DeviceKind.desktop,
      platform: 'macos',
      name: 'Laptop $id',
      status: status,
    );

void main() {
  late FakeDeviceApi api;
  late ProviderContainer container;

  setUp(() {
    api = FakeDeviceApi();
    container = ProviderContainer(
      overrides: [deviceApiProvider.overrideWithValue(api)],
    );
  });
  tearDown(() => container.dispose());

  test('loads devices on build', () async {
    api.devices = [_device('1'), _device('2')];
    final devices = await container.read(devicesControllerProvider.future);
    expect(devices, hasLength(2));
  });

  test('surfaces API errors as an error state', () async {
    api.error = const ApiException(code: 'server_error', message: 'boom');
    // Keep the provider alive and let the async build settle into an error.
    final sub = container.listen(devicesControllerProvider, (_, _) {});
    addTearDown(sub.close);
    await Future<void>.delayed(const Duration(milliseconds: 50));
    expect(container.read(devicesControllerProvider).hasError, isTrue);
  });

  test('remove deletes optimistically and calls the API', () async {
    api.devices = [_device('1'), _device('2')];
    await container.read(devicesControllerProvider.future);

    await container.read(devicesControllerProvider.notifier).remove('1');

    expect(api.deleted, contains('1'));
    final remaining = container.read(devicesControllerProvider).value!;
    expect(remaining.map((d) => d.id), ['2']);
  });

  test('isControllable only for online desktops', () {
    expect(_device('1', status: DeviceStatus.online).isControllable, isTrue);
    expect(_device('1').isControllable, isFalse);
  });
}
