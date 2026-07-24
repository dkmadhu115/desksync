import 'dart:convert';

import 'package:desksync_mobile/core/network/certificate_pinning.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('CertificatePinner', () {
    test('pinForDer is base64 SHA-256 (matches the Rust CertPinner vector)', () {
      // SHA-256("") base64 — identical constant asserted in the agent's
      // desksync-crypto pinning test.
      expect(
        CertificatePinner.pinForDer(const []),
        '47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU=',
      );
    });

    test('trusts a certificate whose pin is configured', () {
      final der = utf8.encode('a fake certificate');
      final pin = CertificatePinner.pinForDer(der);
      final pinner = CertificatePinner(['someotherpin', pin]);
      expect(pinner.isTrusted(der), isTrue);
    });

    test('rejects an unpinned certificate', () {
      final pinner = CertificatePinner([CertificatePinner.pinForDer(utf8.encode('trusted'))]);
      expect(pinner.isTrusted(utf8.encode('rogue')), isFalse);
    });

    test('fails closed when no pins are configured', () {
      const pinner = CertificatePinner([]);
      expect(pinner.isConfigured, isFalse);
      expect(pinner.isTrusted(utf8.encode('anything')), isFalse);
    });
  });
}
