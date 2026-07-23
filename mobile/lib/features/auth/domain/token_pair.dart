/// An OAuth2-style token pair returned by the auth service, mirroring the
/// `TokenPair` schema in the OpenAPI contract.
class TokenPair {
  /// Creates a token pair.
  const TokenPair({
    required this.accessToken,
    required this.refreshToken,
    required this.tokenType,
    required this.expiresIn,
  });

  /// Short-lived JWT access token.
  final String accessToken;

  /// Long-lived, single-use (rotated) refresh token.
  final String refreshToken;

  /// Token type, typically `Bearer`.
  final String tokenType;

  /// Access-token TTL in seconds.
  final int expiresIn;

  /// Parse from the backend JSON body.
  factory TokenPair.fromJson(Map<String, dynamic> json) {
    return TokenPair(
      accessToken: json['access_token'] as String,
      refreshToken: json['refresh_token'] as String,
      tokenType: (json['token_type'] as String?) ?? 'Bearer',
      expiresIn: (json['expires_in'] as num?)?.toInt() ?? 0,
    );
  }
}
