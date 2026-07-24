import 'package:desksync_mobile/features/pairing/domain/pairing.dart';
import 'package:flutter_test/flutter_test.dart';

Pairing pairing({
  required String id,
  required String desktop,
  PairingStatus status = PairingStatus.active,
  DateTime? createdAt,
}) {
  return Pairing(
    id: id,
    mobileDeviceId: 'mobile-1',
    desktopDeviceId: desktop,
    status: status,
    trusted: true,
    createdAt: createdAt,
  );
}

void main() {
  test('returns null when no pairing matches the device', () {
    final result = selectActivePairing(
      [pairing(id: 'p1', desktop: 'other')],
      'desk-1',
    );
    expect(result, isNull);
  });

  test('ignores non-active pairings for the device', () {
    final result = selectActivePairing(
      [
        pairing(id: 'p1', desktop: 'desk-1', status: PairingStatus.revoked),
        pairing(id: 'p2', desktop: 'desk-1', status: PairingStatus.pending),
      ],
      'desk-1',
    );
    expect(result, isNull);
  });

  test('returns the active pairing for the device', () {
    final result = selectActivePairing(
      [
        pairing(id: 'p1', desktop: 'other'),
        pairing(id: 'p2', desktop: 'desk-1'),
      ],
      'desk-1',
    );
    expect(result?.id, 'p2');
  });

  test('prefers the most recently created active pairing', () {
    final result = selectActivePairing(
      [
        pairing(
          id: 'old',
          desktop: 'desk-1',
          createdAt: DateTime.utc(2024, 1, 1),
        ),
        pairing(
          id: 'new',
          desktop: 'desk-1',
          createdAt: DateTime.utc(2024, 6, 1),
        ),
      ],
      'desk-1',
    );
    expect(result?.id, 'new');
  });
}
