import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../devices/application/device_identity.dart';
import '../domain/pairing.dart';
import 'pairing_api.dart';

/// Repository over the pairing service. Confirming a pairing requires this
/// phone to be a registered device, so it resolves (and registers on first use)
/// the device identity before calling the backend.
class PairingRepository {
  /// Creates the repository.
  PairingRepository(this._api, this._identity);

  final PairingApi _api;
  final DeviceIdentity _identity;

  /// Start a pairing with a desktop device.
  Future<PairingChallenge> initiate(String desktopDeviceId) =>
      _api.initiate(desktopDeviceId);

  /// Resolve the active, usable pairing this phone has with [desktopDeviceId],
  /// or null if none exists. A session can only be started over an active
  /// pairing, so the viewer uses this to find the pairing id to connect with.
  Future<Pairing?> activePairingForDevice(String desktopDeviceId) async {
    final pairings = await _api.list();
    return selectActivePairing(pairings, desktopDeviceId);
  }

  /// Confirm a pairing using the desktop-provided code, attaching this device's
  /// registered id as the mobile side of the pairing.
  Future<Pairing> confirm({
    required String pairingId,
    required String code,
  }) async {
    final mobileDeviceId = await _identity.ensureRegistered();
    return _api.confirm(
      pairingId: pairingId,
      code: code,
      mobileDeviceId: mobileDeviceId,
    );
  }
}

/// Provides the [PairingRepository].
final pairingRepositoryProvider = Provider<PairingRepository>((ref) {
  return PairingRepository(
    ref.watch(pairingApiProvider),
    ref.watch(deviceIdentityProvider),
  );
});
