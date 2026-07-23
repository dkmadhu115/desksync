import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Keys used in secure storage. Centralized to avoid typos.
abstract final class StorageKeys {
  static const accessToken = 'access_token';
  static const refreshToken = 'refresh_token';
  static const devicePrivateKey = 'device_private_key';
  static const mobileDeviceId = 'mobile_device_id';
}

/// Abstraction over encrypted key/value storage for the small set of secrets
/// the app persists (auth tokens, the device private key which never leaves the
/// device, and the local device id).
///
/// It is an interface so tests can substitute an in-memory implementation
/// without touching the platform keychain.
abstract interface class SecureStore {
  /// Read a value by key, or null when absent.
  Future<String?> read(String key);

  /// Write a value.
  Future<void> write(String key, String value);

  /// Delete a value.
  Future<void> delete(String key);

  /// Wipe all stored secrets (e.g. on logout or device revocation).
  Future<void> clear();

  /// Persist the access + refresh token pair.
  Future<void> saveTokens({
    required String accessToken,
    required String refreshToken,
  });

  /// Read the persisted access token, if any.
  Future<String?> readAccessToken();

  /// Read the persisted refresh token, if any.
  Future<String?> readRefreshToken();

  /// Remove only the token pair (keeps the device key), e.g. on logout.
  Future<void> clearTokens();
}

/// Default [SecureStore] backed by [FlutterSecureStorage] (Keychain on iOS,
/// EncryptedSharedPreferences/Keystore on Android).
class FlutterSecureStore implements SecureStore {
  /// Creates a store over the given [FlutterSecureStorage].
  const FlutterSecureStore(this._storage);

  final FlutterSecureStorage _storage;

  @override
  Future<String?> read(String key) => _storage.read(key: key);

  @override
  Future<void> write(String key, String value) =>
      _storage.write(key: key, value: value);

  @override
  Future<void> delete(String key) => _storage.delete(key: key);

  @override
  Future<void> clear() => _storage.deleteAll();

  @override
  Future<void> saveTokens({
    required String accessToken,
    required String refreshToken,
  }) async {
    await _storage.write(key: StorageKeys.accessToken, value: accessToken);
    await _storage.write(key: StorageKeys.refreshToken, value: refreshToken);
  }

  @override
  Future<String?> readAccessToken() =>
      _storage.read(key: StorageKeys.accessToken);

  @override
  Future<String?> readRefreshToken() =>
      _storage.read(key: StorageKeys.refreshToken);

  @override
  Future<void> clearTokens() async {
    await _storage.delete(key: StorageKeys.accessToken);
    await _storage.delete(key: StorageKeys.refreshToken);
  }
}

/// Provides a singleton [SecureStore]. Overridden in tests with an in-memory
/// fake.
final secureStoreProvider = Provider<SecureStore>((ref) {
  return const FlutterSecureStore(
    FlutterSecureStorage(
      iOptions: IOSOptions(accessibility: KeychainAccessibility.first_unlock),
    ),
  );
});
