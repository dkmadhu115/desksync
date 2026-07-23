import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../../../core/network/dio_client.dart';
import '../domain/token_pair.dart';

/// Thin transport over the auth service REST endpoints. It only performs HTTP
/// and JSON (de)serialization, mapping failures to [ApiException]; persistence
/// and state live in the repository/controller.
class AuthApi {
  /// Creates the API over a configured [Dio].
  AuthApi(this._dio);

  final Dio _dio;

  /// POST /api/v1/auth/login
  Future<TokenPair> login(String email, String password) async {
    return _tokenCall('/api/v1/auth/login', {
      'email': email,
      'password': password,
    });
  }

  /// POST /api/v1/auth/register
  Future<TokenPair> register(
    String email,
    String password, {
    String? displayName,
  }) async {
    return _tokenCall('/api/v1/auth/register', {
      'email': email,
      'password': password,
      if (displayName != null && displayName.isNotEmpty)
        'display_name': displayName,
    });
  }

  /// POST /api/v1/auth/logout — best-effort revocation of the refresh token.
  Future<void> logout(String refreshToken) async {
    try {
      await _dio.post<void>(
        '/api/v1/auth/logout',
        data: {'refresh_token': refreshToken},
      );
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }

  Future<TokenPair> _tokenCall(String path, Map<String, dynamic> body) async {
    try {
      final resp = await _dio.post<Map<String, dynamic>>(path, data: body);
      final data = resp.data;
      if (data == null) {
        throw const ApiException(
          code: 'server_error',
          message: 'The server returned an empty response.',
        );
      }
      return TokenPair.fromJson(data);
    } on DioException catch (e) {
      throw ApiException.fromDio(e);
    }
  }
}

/// Provides the [AuthApi].
final authApiProvider = Provider<AuthApi>((ref) {
  return AuthApi(ref.watch(dioProvider));
});
