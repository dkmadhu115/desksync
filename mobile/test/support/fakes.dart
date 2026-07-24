import 'package:desksync_mobile/core/network/api_exception.dart';
import 'package:desksync_mobile/core/storage/secure_storage.dart';
import 'package:desksync_mobile/features/auth/data/auth_api.dart';
import 'package:desksync_mobile/features/auth/domain/token_pair.dart';
import 'package:desksync_mobile/features/devices/data/device_api.dart';
import 'package:desksync_mobile/features/devices/domain/device.dart';
import 'package:desksync_mobile/features/viewer/application/input_sink.dart';
import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:dio/dio.dart';

/// In-memory [SecureStore] for tests (no platform keychain).
class InMemorySecureStore implements SecureStore {
  final Map<String, String> _data = {};

  @override
  Future<String?> read(String key) async => _data[key];

  @override
  Future<void> write(String key, String value) async => _data[key] = value;

  @override
  Future<void> delete(String key) async => _data.remove(key);

  @override
  Future<void> clear() async => _data.clear();

  @override
  Future<void> saveTokens({
    required String accessToken,
    required String refreshToken,
  }) async {
    _data[StorageKeys.accessToken] = accessToken;
    _data[StorageKeys.refreshToken] = refreshToken;
  }

  @override
  Future<String?> readAccessToken() async => _data[StorageKeys.accessToken];

  @override
  Future<String?> readRefreshToken() async => _data[StorageKeys.refreshToken];

  @override
  Future<void> clearTokens() async {
    _data.remove(StorageKeys.accessToken);
    _data.remove(StorageKeys.refreshToken);
  }
}

/// Configurable fake auth API.
class FakeAuthApi extends AuthApi {
  FakeAuthApi() : super(Dio());

  /// When set, calls throw this instead of succeeding.
  ApiException? error;

  /// Tokens returned on success.
  TokenPair tokens = const TokenPair(
    accessToken: 'access-123',
    refreshToken: 'refresh-123',
    tokenType: 'Bearer',
    expiresIn: 900,
  );

  int logoutCalls = 0;

  @override
  Future<TokenPair> login(String email, String password) async {
    if (error != null) throw error!;
    return tokens;
  }

  @override
  Future<TokenPair> register(
    String email,
    String password, {
    String? displayName,
  }) async {
    if (error != null) throw error!;
    return tokens;
  }

  @override
  Future<void> logout(String refreshToken) async {
    logoutCalls++;
  }
}

/// Configurable fake device API.
class FakeDeviceApi extends DeviceApi {
  FakeDeviceApi() : super(Dio());

  List<Device> devices = [];
  ApiException? error;
  final List<String> deleted = [];

  /// Registrations received, and how many times [register] was called.
  final List<DeviceRegistration> registrations = [];
  int registerCalls = 0;

  /// Id assigned to the next registered device.
  String nextRegisteredId = 'mobile-dev-1';

  @override
  Future<Device> register(DeviceRegistration registration) async {
    if (error != null) throw error!;
    registerCalls++;
    registrations.add(registration);
    final device = Device(
      id: nextRegisteredId,
      kind: registration.kind,
      platform: registration.platform,
      name: registration.name,
      status: DeviceStatus.offline,
    );
    devices = [...devices, device];
    return device;
  }

  @override
  Future<List<Device>> list() async {
    if (error != null) throw error!;
    return devices;
  }

  @override
  Future<void> delete(String deviceId) async {
    if (error != null) throw error!;
    deleted.add(deviceId);
    devices = devices.where((d) => d.id != deviceId).toList();
  }
}

/// Records input events for assertions.
class RecordingInputSink implements InputSink {
  final List<InputEvent> events = [];

  @override
  void send(InputEvent event) => events.add(event);

  @override
  void sendAll(Iterable<InputEvent> events) => this.events.addAll(events);
}
