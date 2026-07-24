import 'package:desksync_mobile/core/storage/secure_storage.dart';
import 'package:desksync_mobile/features/devices/application/device_identity.dart';
import 'package:desksync_mobile/features/devices/domain/device.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

void main() {
  group('DeviceIdentity.ensureRegistered', () {
    test('registers once and caches the server-assigned id', () async {
      final api = FakeDeviceApi()..nextRegisteredId = 'srv-42';
      final store = InMemorySecureStore();
      final identity = DeviceIdentity(api, store);

      final id = await identity.ensureRegistered();
      expect(id, 'srv-42');
      expect(api.registerCalls, 1);

      // Registered as a mobile device with a base64 public key.
      final reg = api.registrations.single;
      expect(reg.kind, DeviceKind.mobile);
      expect(reg.publicKey, isNotEmpty);

      // The id is persisted, and the public key is stored for reuse.
      expect(await store.read(StorageKeys.mobileDeviceId), 'srv-42');
      expect(await store.read(StorageKeys.mobileDevicePublicKey), isNotEmpty);

      // A second call returns the cached id without re-registering.
      final again = await identity.ensureRegistered();
      expect(again, 'srv-42');
      expect(api.registerCalls, 1);
    });

    test('does not re-register when an id already exists', () async {
      final api = FakeDeviceApi();
      final store = InMemorySecureStore()
        ..write(StorageKeys.mobileDeviceId, 'existing-id');
      final identity = DeviceIdentity(api, store);

      final id = await identity.ensureRegistered();
      expect(id, 'existing-id');
      expect(api.registerCalls, 0);
    });
  });
}
