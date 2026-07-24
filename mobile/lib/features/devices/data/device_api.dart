import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../../../core/network/dio_client.dart';
import '../domain/device.dart';

/// REST transport for the device service.
class DeviceApi {
  /// Creates the API over a configured [Dio].
  DeviceApi(this._dio);

  final Dio _dio;

  /// POST /api/v1/devices — register (or idempotently re-register) a device.
  Future<Device> register(DeviceRegistration registration) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(
        '/api/v1/devices',
        data: registration.toJson(),
      );
      return Device.fromJson(resp.data!);
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  /// GET /api/v1/devices
  Future<List<Device>> list() async {
    try {
      final resp = await _dio.get<List<dynamic>>('/api/v1/devices');
      final items = resp.data ?? const [];
      return items
          .whereType<Map<String, dynamic>>()
          .map(Device.fromJson)
          .toList(growable: false);
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  /// DELETE /api/v1/devices/{id} — revoke and remove a device.
  Future<void> delete(String deviceId) async {
    try {
      await _dio.delete<void>('/api/v1/devices/$deviceId');
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }
}

/// Provides the [DeviceApi].
final deviceApiProvider = Provider<DeviceApi>((ref) {
  return DeviceApi(ref.watch(dioProvider));
});
