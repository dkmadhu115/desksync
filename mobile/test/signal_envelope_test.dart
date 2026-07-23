import 'dart:convert';

import 'package:desksync_mobile/features/signaling/domain/signal_envelope.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('offer envelope serializes to the backend wire shape', () {
    const env = SignalEnvelope(
      sessionId: 'sess-1',
      nonce: 7,
      tsMs: 123,
      payload: OfferPayload(sdp: 'v=0'),
    );
    final json = env.toJson();
    expect(json, {
      'v': 1,
      'nonce': 7,
      'ts_ms': 123,
      'session_id': 'sess-1',
      'payload': {'kind': 'offer', 'sdp': 'v=0'},
    });
    // Round-trips through a JSON string.
    final back = SignalEnvelope.fromJson(
      jsonDecode(jsonEncode(json)) as Map<String, dynamic>,
    );
    expect(back.nonce, 7);
    expect(back.payload, isA<OfferPayload>());
  });

  test('ice_candidate uses snake_case sdp_m_line_index', () {
    const env = SignalEnvelope(
      sessionId: 's',
      nonce: 1,
      payload: IceCandidatePayload(candidate: 'cand', sdpMLineIndex: 2),
    );
    expect(env.toJson()['payload'], {
      'kind': 'ice_candidate',
      'candidate': 'cand',
      'sdp_m_line_index': 2,
    });
  });

  test('parses server presence control messages', () {
    final joined = SignalPayload.fromJson({'kind': 'peer_joined', 'role': 'agent'});
    expect(joined, isA<PeerJoinedPayload>());
    expect((joined as PeerJoinedPayload).role, 'agent');

    final left = SignalPayload.fromJson({'kind': 'peer_left', 'role': 'agent'});
    expect(left, isA<PeerLeftPayload>());
  });

  test('unknown payload kinds are tolerated', () {
    final p = SignalPayload.fromJson({'kind': 'future_thing', 'x': 1});
    expect(p, isA<UnknownPayload>());
    expect(p.kind, 'future_thing');
  });

  test('answer and bye/heartbeat round-trip', () {
    expect(
      SignalPayload.fromJson({'kind': 'answer', 'sdp': 'a'}),
      isA<AnswerPayload>(),
    );
    expect(SignalPayload.fromJson({'kind': 'bye'}), isA<ByePayload>());
    expect(
      SignalPayload.fromJson({'kind': 'heartbeat'}),
      isA<HeartbeatPayload>(),
    );
  });
}
