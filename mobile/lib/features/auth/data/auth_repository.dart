import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/storage/secure_storage.dart';
import '../domain/token_pair.dart';
import 'auth_api.dart';

/// Coordinates the auth API with secure token persistence. The controller talks
/// only to this repository, which keeps token storage in one place and makes
/// the flows easy to unit-test with a fake.
class AuthRepository {
  /// Creates the repository.
  AuthRepository(this._api, this._store);

  final AuthApi _api;
  final SecureStore _store;

  /// Log in and persist the resulting tokens.
  Future<TokenPair> login(String email, String password) async {
    final tokens = await _api.login(email, password);
    await _persist(tokens);
    return tokens;
  }

  /// Register a new account and persist the resulting tokens.
  Future<TokenPair> register(
    String email,
    String password, {
    String? displayName,
  }) async {
    final tokens = await _api.register(email, password, displayName: displayName);
    await _persist(tokens);
    return tokens;
  }

  /// Revoke the refresh token server-side (best effort) and clear local tokens.
  Future<void> logout() async {
    final refresh = await _store.readRefreshToken();
    try {
      if (refresh != null && refresh.isNotEmpty) {
        await _api.logout(refresh);
      }
    } finally {
      await _store.clearTokens();
    }
  }

  /// Whether a persisted access token exists (used to bootstrap on launch).
  Future<bool> hasValidSession() async {
    final token = await _store.readAccessToken();
    return token != null && token.isNotEmpty;
  }

  /// Clear tokens locally without a server round-trip (e.g. on 401).
  Future<void> clearLocalSession() => _store.clearTokens();

  Future<void> _persist(TokenPair tokens) => _store.saveTokens(
        accessToken: tokens.accessToken,
        refreshToken: tokens.refreshToken,
      );
}

/// Provides the [AuthRepository].
final authRepositoryProvider = Provider<AuthRepository>((ref) {
  return AuthRepository(
    ref.watch(authApiProvider),
    ref.watch(secureStoreProvider),
  );
});
