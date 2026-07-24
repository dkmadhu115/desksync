import 'package:desksync_mobile/features/session/domain/session.dart';
import 'package:desksync_mobile/features/viewer/application/input_sink.dart';
import 'package:desksync_mobile/features/viewer/application/viewer_controller.dart';
import 'package:desksync_mobile/features/viewer/application/webrtc_session.dart';
import 'package:flutter_test/flutter_test.dart';

WebRtcSession _neverBuild(SessionCreated created) =>
    throw StateError('session factory should not be called');

Future<SessionCreated> _neverCreate(String pairingId) =>
    throw StateError('createSession should not be called');

void main() {
  test('reports noPairing when the device has no active pairing', () async {
    final controller = ViewerController(
      deviceId: 'desk-1',
      resolvePairing: (_) async => null,
      createSession: _neverCreate,
      sessionFactory: _neverBuild,
      inputSink: SwitchableInputSink(),
    );

    await controller.connect();

    expect(controller.phase, ViewerPhase.noPairing);
    expect(controller.renderer, isNull);
  });

  test('fails when pairing resolution throws', () async {
    final controller = ViewerController(
      deviceId: 'desk-1',
      resolvePairing: (_) async => throw Exception('network down'),
      createSession: _neverCreate,
      sessionFactory: _neverBuild,
      inputSink: SwitchableInputSink(),
    );

    await controller.connect();

    expect(controller.phase, ViewerPhase.failed);
    expect(controller.errorMessage, contains('network down'));
  });

  test('does not attach the input sink before connecting', () {
    final sink = SwitchableInputSink();
    ViewerController(
      deviceId: 'desk-1',
      resolvePairing: (_) async => null,
      createSession: _neverCreate,
      sessionFactory: _neverBuild,
      inputSink: sink,
    );
    expect(sink.hasTarget, isFalse);
  });
}
