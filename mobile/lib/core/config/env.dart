/// Compile-time environment configuration.
///
/// Values are provided via `--dart-define` at build time so no secrets are
/// baked into source. Defaults target a local backend for development.
abstract final class Env {
  /// Base URL of the API gateway.
  static const String apiBaseUrl = String.fromEnvironment(
    'DESKSYNC_API_BASE_URL',
    defaultValue: 'http://localhost:8080',
  );

  /// Secure WebSocket URL of the signaling service.
  static const String signalingUrl = String.fromEnvironment(
    'DESKSYNC_SIGNALING_URL',
    defaultValue: 'ws://localhost:8085/api/v1/signaling',
  );

  /// Request timeout in milliseconds.
  static const int requestTimeoutMs = int.fromEnvironment(
    'DESKSYNC_REQUEST_TIMEOUT_MS',
    defaultValue: 15000,
  );

  /// Comma-separated base64 SHA-256 leaf-certificate pins for the API gateway.
  /// Empty (the default) disables pinning for local development; production
  /// builds supply pins via `--dart-define`.
  static const String certPins = String.fromEnvironment(
    'DESKSYNC_CERT_PINS',
    defaultValue: '',
  );

  /// Parsed, non-empty certificate pins.
  static List<String> get certPinSet => certPins
      .split(',')
      .map((p) => p.trim())
      .where((p) => p.isNotEmpty)
      .toList(growable: false);
}
