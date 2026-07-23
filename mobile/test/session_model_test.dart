import 'package:desksync_mobile/features/session/domain/session.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  test('SessionCreated parses session, url, ticket, and ICE servers', () {
    final json = {
      'session': {
        'id': 'sess-1',
        'pairing_id': 'pair-1',
        'status': 'connecting',
        'connection_type': null,
        'started_at': '2026-07-23T10:00:00Z',
        'ended_at': null,
      },
      'signaling_url': 'wss://example.com/api/v1/signaling/ws',
      'signaling_ticket': 'v1.abc.def',
      'ice_servers': [
        {'urls': ['stun:stun.example.com:3478']},
        {
          'urls': ['turn:turn.example.com:3478'],
          'username': '1700:sess-1',
          'credential': 'secretcred',
        },
      ],
    };

    final created = SessionCreated.fromJson(json);
    expect(created.session.id, 'sess-1');
    expect(created.session.status, SessionStatus.connecting);
    expect(created.signalingUrl, 'wss://example.com/api/v1/signaling/ws');
    expect(created.signalingTicket, 'v1.abc.def');
    expect(created.iceServers.length, 2);
    expect(created.iceServers[0].username, isNull);
    expect(created.iceServers[1].username, '1700:sess-1');
  });

  test('toRtcConfiguration produces flutter_webrtc-shaped config', () {
    final created = SessionCreated(
      session: const Session(
        id: 's',
        pairingId: 'p',
        status: SessionStatus.connecting,
      ),
      signalingUrl: 'wss://x/ws',
      signalingTicket: 't',
      iceServers: const [
        IceServer(urls: ['turn:t:3478'], username: 'u', credential: 'c'),
      ],
    );
    final cfg = created.toRtcConfiguration();
    expect(cfg['sdpSemantics'], 'unified-plan');
    final servers = cfg['iceServers'] as List;
    expect(servers.first, {
      'urls': ['turn:t:3478'],
      'username': 'u',
      'credential': 'c',
    });
  });

  test('unknown status falls back to unknown', () {
    final s = Session.fromJson({
      'id': 's',
      'pairing_id': 'p',
      'status': 'weird',
    });
    expect(s.status, SessionStatus.unknown);
  });
}
