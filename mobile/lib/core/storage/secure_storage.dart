import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_secure_storage/flutter_secure_storage.dart';

/// Keys used in secure storage. Centralized to avoid typos.
abstract final class StorageKeys {
  static const accessToken = 'access_token';
  static const refreshToken = 'refresh_token';
  static const devicePrivateKey = 'device_private_key';
}

/// Thin wrapper over [FlutterSecureStorage] exposing typed helpers for the
/// small set of secrets the app persists (tokens and the device private key,
/// which never leaves the device).
class SecureStore {
  /// Creates a store backed by the given [FlutterSecureStorage].
  const SecureStore(this._storage);

  final FlutterSecureStorage _storage;

  /// Read a value by key, or null when absent.
  Future<String?> read(String key) => _storage.read(key: key);

  /// Write a value.
  Future<void> write(String key, String value) =>
      _storage.write(key: key, value: value);

  /// Delete a value.
  Future<void> delete(String key) => _storage.delete(key: key);

  /// Wipe all stored secrets (e.g. on logout or device revocation).
  Future<void> clear() => _storage.deleteAll();
}

/// Provides a singleton [SecureStore]. Overridden in tests with a fake.
final secureStoreProvider = Provider<SecureStore>((ref) {
  return const SecureStore(
    FlutterSecureStorage(
      iOptions: IOSOptions(accessibility: KeychainAccessibility.first_unlock),
    ),
  );
});
