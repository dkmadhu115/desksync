import 'dart:convert';
import 'dart:typed_data';

import 'package:desksync_mobile/features/security/secure_channel.dart';
import 'package:flutter_test/flutter_test.dart';

List<int> _hex(String s) {
  final out = <int>[];
  for (var i = 0; i < s.length; i += 2) {
    out.add(int.parse(s.substring(i, i + 2), radix: 16));
  }
  return out;
}

String _hexOf(List<int> b) =>
    b.map((x) => x.toRadixString(16).padLeft(2, '0')).join();

void main() {
  // Shared constants mirrored from the Rust `desksync-crypto` interop vector.
  final shared = List<int>.filled(32, 3);
  const sessionId = 'sess-vector';
  final controllerPub = List<int>.filled(32, 1);
  final agentPub = List<int>.filled(32, 2);
  const frameHex = '00000000000000001332232a849ee705233318f10f25aa7f7c39c00726';

  group('SecureChannel interop with the Rust agent', () {
    test('controller seal of counter 0 matches the Rust frame byte-for-byte',
        () async {
      final controller = await SecureChannel.fromSharedSecret(
        sharedSecret: shared,
        sessionId: sessionId,
        controllerPub: controllerPub,
        agentPub: agentPub,
        role: SecureRole.controller,
      );
      final frame = await controller.seal(utf8.encode('hello'));
      expect(_hexOf(frame), frameHex);
    });

    test('agent opens the Rust-produced frame', () async {
      final agent = await SecureChannel.fromSharedSecret(
        sharedSecret: shared,
        sessionId: sessionId,
        controllerPub: controllerPub,
        agentPub: agentPub,
        role: SecureRole.agent,
      );
      final clear = await agent.open(Uint8List.fromList(_hex(frameHex)));
      expect(utf8.decode(clear), 'hello');
    });
  });

  group('SecureChannel round-trip', () {
    Future<(SecureChannel, SecureChannel)> pair() async {
      final s = List<int>.filled(32, 9);
      final c = await SecureChannel.fromSharedSecret(
        sharedSecret: s,
        sessionId: 'sess',
        controllerPub: controllerPub,
        agentPub: agentPub,
        role: SecureRole.controller,
      );
      final a = await SecureChannel.fromSharedSecret(
        sharedSecret: s,
        sessionId: 'sess',
        controllerPub: controllerPub,
        agentPub: agentPub,
        role: SecureRole.agent,
      );
      return (c, a);
    }

    test('encrypts in both directions', () async {
      final (controller, agent) = await pair();
      final f = await controller.seal(utf8.encode('move 10 20'));
      expect(utf8.decode(await agent.open(f)), 'move 10 20');
      final g = await agent.seal(utf8.encode('clip'));
      expect(utf8.decode(await controller.open(g)), 'clip');
    });

    test('rejects replayed and out-of-order frames', () async {
      final (controller, agent) = await pair();
      final f1 = await controller.seal(utf8.encode('one'));
      final f2 = await controller.seal(utf8.encode('two'));
      await agent.open(f2);
      expect(
        () => agent.open(f2),
        throwsA(isA<SecureChannelException>()
            .having((e) => e.error, 'error', SecureChannelError.replay)),
      );
      expect(
        () => agent.open(f1),
        throwsA(isA<SecureChannelException>()
            .having((e) => e.error, 'error', SecureChannelError.replay)),
      );
    });

    test('rejects tampered frames', () async {
      final (controller, agent) = await pair();
      final f = Uint8List.fromList(await controller.seal(utf8.encode('secret')));
      f[f.length - 1] ^= 0x01;
      expect(
        () => agent.open(f),
        throwsA(isA<SecureChannelException>()
            .having((e) => e.error, 'error', SecureChannelError.authentication)),
      );
    });

    test('rejects short frames as malformed', () async {
      final (_, agent) = await pair();
      expect(
        () => agent.open(Uint8List(4)),
        throwsA(isA<SecureChannelException>()
            .having((e) => e.error, 'error', SecureChannelError.malformed)),
      );
    });
  });
}
