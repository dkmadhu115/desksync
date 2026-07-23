import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/storage/secure_storage.dart';
import '../../../core/util/uuid.dart';
import '../domain/pairing.dart';
import 'pairing_api.dart';

/// Repository over the pairing service. It also owns the persistent local
/// mobile-device identifier used when confirming a pairing.
class PairingRepository {
  /// Creates the repository.
  PairingRepository(this._api, this._store);

  final PairingApi _api;
  final SecureStore _store;

  /// Start a pairing with a desktop device.
  Future<PairingChallenge> initiate(String desktopDeviceId) =>
      _api.initiate(desktopDeviceId);

  /// Confirm a pairing using the desktop-provided code, attaching this device's
  /// stable local id.
  Future<Pairing> confirm({
    required String pairingId,
    required String code,
  }) async {
    final mobileDeviceId = await mobileDeviceId0();
    return _api.confirm(
      pairingId: pairingId,
      code: code,
      mobileDeviceId: mobileDeviceId,
    );
  }

  /// Return this device's persistent local id, creating and storing one on
  /// first use.
  Future<String> mobileDeviceId0() async {
    final existing = await _store.read(StorageKeys.mobileDeviceId);
    if (existing != null && existing.isNotEmpty) return existing;
    final id = generateUuidV4();
    await _store.write(StorageKeys.mobileDeviceId, id);
    return id;
  }
}

/// Provides the [PairingRepository].
final pairingRepositoryProvider = Provider<PairingRepository>((ref) {
  return PairingRepository(
    ref.watch(pairingApiProvider),
    ref.watch(secureStoreProvider),
  );
});
