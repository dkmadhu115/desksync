import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../../../core/network/dio_client.dart';
import '../domain/pairing.dart';

/// REST transport for the pairing service.
class PairingApi {
  /// Creates the API over a configured [Dio].
  PairingApi(this._dio);

  final Dio _dio;

  /// POST /api/v1/pairing/initiate
  Future<PairingChallenge> initiate(String desktopDeviceId) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(
        '/api/v1/pairing/initiate',
        data: {'desktop_device_id': desktopDeviceId},
      );
      return PairingChallenge.fromJson(resp.data!);
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  /// GET /api/v1/pairings — list the caller's persistent pairings.
  Future<List<Pairing>> list() async {
    try {
      final resp = await _dio.get<List<dynamic>>('/api/v1/pairings');
      final data = resp.data ?? const [];
      return data
          .whereType<Map<String, dynamic>>()
          .map(Pairing.fromJson)
          .toList();
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  /// POST /api/v1/pairing/confirm
  Future<Pairing> confirm({
    required String pairingId,
    required String code,
    required String mobileDeviceId,
  }) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(
        '/api/v1/pairing/confirm',
        data: {
          'pairing_id': pairingId,
          'code': code,
          'mobile_device_id': mobileDeviceId,
        },
      );
      return Pairing.fromJson(resp.data!);
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }
}

/// Provides the [PairingApi].
final pairingApiProvider = Provider<PairingApi>((ref) {
  return PairingApi(ref.watch(dioProvider));
});
