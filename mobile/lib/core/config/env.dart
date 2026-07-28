/// Compile-time environment configuration.
///
/// Values are provided via `--dart-define` at build time so no secrets are
/// baked into source. The defaults are the hosted service, so a release build
/// with no defines at all is still a working app; override them to point at a
/// backend you are running yourself.
abstract final class Env {
  /// Base URL of the API gateway.
  ///
  /// The only endpoint the app needs configured. The signaling WebSocket URL is
  /// not compiled in: each session's response carries its own `signaling_url`
  /// (from the backend's `SIGNALING_PUBLIC_URL`), so the two can never disagree
  /// and a backend can move its signaling service without reissuing the app.
  static const String apiBaseUrl = String.fromEnvironment(
    'DESKSYNC_API_BASE_URL',
    defaultValue: 'https://dkmadhutech.com',
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
