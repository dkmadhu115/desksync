import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../../../core/network/dio_client.dart';
import '../domain/session.dart';

/// REST transport for the session service.
class SessionApi {
  /// Creates the API over a configured [Dio].
  SessionApi(this._dio);

  final Dio _dio;

  /// POST /api/v1/sessions — create a session for a pairing and obtain the
  /// signaling URL/ticket and ICE configuration.
  Future<SessionCreated> create(String pairingId) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(
        '/api/v1/sessions',
        data: {'pairing_id': pairingId},
      );
      return SessionCreated.fromJson(resp.data ?? const {});
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  /// POST /api/v1/sessions/{id}/end — end a session (idempotent).
  Future<Session> end(String sessionId) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(
        '/api/v1/sessions/$sessionId/end',
      );
      return Session.fromJson(resp.data ?? const {});
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }
}

/// Provides the [SessionApi].
final sessionApiProvider = Provider<SessionApi>((ref) {
  return SessionApi(ref.watch(dioProvider));
});
