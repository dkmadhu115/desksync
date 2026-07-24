/// A parsed DeskSync pairing deep link.
///
/// The desktop encodes a QR code as `desksync://pair?v=1&pid=<id>&code=<code>`.
/// [tryParse] extracts the pairing id and code, tolerating extra/unknown query
/// parameters and rejecting anything that is not a DeskSync pairing link.
class PairingLink {
  /// Creates a pairing link.
  const PairingLink({required this.pairingId, required this.code});

  /// The pairing challenge id to confirm.
  final String pairingId;

  /// The one-time pairing code.
  final String code;

  /// The deep-link scheme.
  static const scheme = 'desksync';

  /// The deep-link host.
  static const host = 'pair';

  /// Parse [raw], returning null when it is not a valid pairing link.
  static PairingLink? tryParse(String raw) {
    final uri = Uri.tryParse(raw.trim());
    if (uri == null) return null;
    if (uri.scheme != scheme || uri.host != host) return null;

    final pid = uri.queryParameters['pid']?.trim() ?? '';
    final code = uri.queryParameters['code']?.trim() ?? '';
    if (pid.isEmpty || code.isEmpty) return null;

    return PairingLink(pairingId: pid, code: code);
  }
}
