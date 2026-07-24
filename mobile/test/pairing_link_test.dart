import 'package:desksync_mobile/features/pairing/domain/pairing_link.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('PairingLink.tryParse', () {
    test('parses a valid desksync pairing link', () {
      final link = PairingLink.tryParse(
        'desksync://pair?v=1&pid=abc-123&code=45678901',
      );
      expect(link, isNotNull);
      expect(link!.pairingId, 'abc-123');
      expect(link.code, '45678901');
    });

    test('tolerates surrounding whitespace and extra params', () {
      final link = PairingLink.tryParse(
        '  desksync://pair?pid=p1&code=00000000&extra=ignored  ',
      );
      expect(link, isNotNull);
      expect(link!.pairingId, 'p1');
      expect(link.code, '00000000');
    });

    test('rejects the wrong scheme', () {
      expect(
        PairingLink.tryParse('https://pair?pid=p1&code=1'),
        isNull,
      );
    });

    test('rejects the wrong host', () {
      expect(
        PairingLink.tryParse('desksync://connect?pid=p1&code=1'),
        isNull,
      );
    });

    test('rejects when pid or code is missing', () {
      expect(PairingLink.tryParse('desksync://pair?pid=p1'), isNull);
      expect(PairingLink.tryParse('desksync://pair?code=1'), isNull);
      expect(PairingLink.tryParse('desksync://pair'), isNull);
    });

    test('rejects garbage input', () {
      expect(PairingLink.tryParse('not a uri at all'), isNull);
      expect(PairingLink.tryParse(''), isNull);
    });
  });
}
