import 'dart:convert';
import 'dart:io';

import 'package:desksync_mobile/features/signaling/data/signaling_client.dart';
import 'package:desksync_mobile/features/signaling/domain/signal_envelope.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('connects, receives presence, and round-trips an echoed offer', () async {
    // A local WebSocket server that announces presence then echoes messages,
    // standing in for the signaling relay.
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    server.listen((HttpRequest req) async {
      final ws = await WebSocketTransformer.upgrade(req);
      ws.add(jsonEncode(const SignalEnvelope(
        sessionId: 'sess-1',
        nonce: 0,
        payload: PeerJoinedPayload(role: 'agent'),
      ).toJson()));
      ws.listen((data) => ws.add(data));
    });

    final client = SignalingClient(
      sessionId: 'sess-1',
      heartbeatInterval: const Duration(hours: 1), // don't fire during the test
    );
    final received = <SignalEnvelope>[];
    final sub = client.messages.listen(received.add);

    final url = 'ws://${server.address.host}:${server.port}/api/v1/signaling/ws';
    await client.connect(url: url, ticket: 'tk', role: 'controller');
    expect(client.isConnected, isTrue);

    // Wait for the presence announcement.
    await _pumpUntil(() => received.any((e) => e.payload is PeerJoinedPayload));

    client.send(const OfferPayload(sdp: 'v=0'));
    await _pumpUntil(() => received.any((e) => e.payload is OfferPayload));

    final offer = received.firstWhere((e) => e.payload is OfferPayload);
    expect((offer.payload as OfferPayload).sdp, 'v=0');
    expect(offer.nonce, 1); // first client-sent message

    await sub.cancel();
    await client.close();
    await server.close(force: true);
  });
}

/// Poll [done] until true or a timeout, yielding to the event loop between
/// checks (needed to drain real socket callbacks).
Future<void> _pumpUntil(
  bool Function() done, {
  Duration timeout = const Duration(seconds: 3),
}) async {
  final deadline = DateTime.now().add(timeout);
  while (!done()) {
    if (DateTime.now().isAfter(deadline)) {
      fail('condition not met within $timeout');
    }
    await Future<void>.delayed(const Duration(milliseconds: 10));
  }
}
