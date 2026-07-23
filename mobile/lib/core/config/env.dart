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
}
