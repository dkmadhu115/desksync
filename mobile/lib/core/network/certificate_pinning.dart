import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:dio/io.dart';

/// Leaf-certificate pinning for the API gateway connection.
///
/// TLS still performs normal chain validation; pinning is an **additional**,
/// fail-closed check that the presented leaf certificate's SHA-256 (base64) is
/// one we expect. This is the Dart counterpart of the agent's
/// `desksync-crypto` `CertPinner` and defends against a mis-issued or
/// rogue-CA certificate (e.g. a corporate MITM proxy) that would otherwise
/// validate.
class CertificatePinner {
  /// Creates a pinner from base64 SHA-256 leaf-cert pins.
  const CertificatePinner(this.pins);

  /// Allowed base64 SHA-256 pins.
  final List<String> pins;

  /// Whether any pins are configured.
  bool get isConfigured => pins.isNotEmpty;

  /// Compute the pin (base64 SHA-256) for a DER-encoded certificate.
  static String pinForDer(List<int> der) => base64.encode(sha256.convert(der).bytes);

  /// Whether the DER certificate matches a configured pin. Fail-closed: returns
  /// false when no pins are configured.
  bool isTrusted(List<int> der) => isConfigured && pins.contains(pinForDer(der));
}

/// Apply [pinner] to a Dio [IOHttpClientAdapter]. When pins are configured, the
/// underlying [HttpClient] rejects any certificate whose leaf pin is not in the
/// set. When unconfigured, the adapter is left at platform defaults.
///
/// Returns the adapter to assign to `dio.httpClientAdapter`.
IOHttpClientAdapter pinningAdapter(CertificatePinner pinner) {
  return IOHttpClientAdapter(
    createHttpClient: () {
      final client = HttpClient();
      if (pinner.isConfigured) {
        // Called when default validation fails (self-signed/MITM); accept only
        // when the leaf matches a pin. Genuine, pinned endpoints that also pass
        // default validation are unaffected.
        client.badCertificateCallback =
            (cert, host, port) => pinner.isTrusted(cert.der);
      }
      return client;
    },
  );
}
