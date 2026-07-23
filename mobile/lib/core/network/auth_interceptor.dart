import 'package:dio/dio.dart';

import '../storage/secure_storage.dart';

/// Dio interceptor that attaches the bearer access token to authenticated
/// requests and transparently refreshes it on `401`.
///
/// Design notes:
/// - It extends [QueuedInterceptor] so request/error handling is serialized;
///   concurrent 401s cannot each kick off their own refresh.
/// - Refresh is additionally guarded for **refresh-token rotation** (the
///   backend rotates the refresh token and detects reuse). Each outgoing
///   request records the access token it used; on 401 we first check whether
///   the stored token has already been rotated by another in-flight refresh and
///   simply retry with the new token, only performing a real refresh when the
///   token is genuinely stale. A single in-flight refresh future is shared.
/// - On refresh failure the session is considered expired: tokens are cleared
///   and [onSessionExpired] is invoked so the app can route back to login.
class AuthInterceptor extends QueuedInterceptor {
  /// Creates the interceptor.
  AuthInterceptor({
    required this.store,
    required this.refreshDio,
    required this.onSessionExpired,
  });

  /// Secure storage holding the token pair.
  final SecureStore store;

  /// A Dio instance **without** this interceptor, used to perform the refresh
  /// call and to replay the original request (avoids recursive interception).
  final Dio refreshDio;

  /// Invoked when refresh fails and the user must re-authenticate.
  final void Function() onSessionExpired;

  static const _retriedKey = '__auth_retried__';
  static const _usedTokenKey = '__auth_used_token__';

  Future<bool>? _inFlightRefresh;

  bool _isPublicAuthPath(String path) {
    return path.contains('/auth/login') ||
        path.contains('/auth/register') ||
        path.contains('/auth/refresh') ||
        path.contains('/auth/oauth');
  }

  @override
  Future<void> onRequest(
    RequestOptions options,
    RequestInterceptorHandler handler,
  ) async {
    if (!_isPublicAuthPath(options.path)) {
      final token = await store.read(StorageKeys.accessToken);
      if (token != null && token.isNotEmpty) {
        options.headers['Authorization'] = 'Bearer $token';
        options.extra[_usedTokenKey] = token;
      }
    }
    handler.next(options);
  }

  @override
  Future<void> onError(
    DioException err,
    ErrorInterceptorHandler handler,
  ) async {
    final status = err.response?.statusCode;
    final options = err.requestOptions;
    final alreadyRetried = options.extra[_retriedKey] == true;

    if (status != 401 ||
        _isPublicAuthPath(options.path) ||
        alreadyRetried) {
      return handler.next(err);
    }

    // If another refresh already rotated the token, just retry with the fresh
    // one instead of refreshing again (which would trip rotation/theft checks).
    final current = await store.read(StorageKeys.accessToken);
    final used = options.extra[_usedTokenKey] as String?;
    final tokenAlreadyRotated =
        current != null && current.isNotEmpty && current != used;

    final refreshed = tokenAlreadyRotated ? true : await _refresh();
    if (!refreshed) {
      await store.delete(StorageKeys.accessToken);
      await store.delete(StorageKeys.refreshToken);
      onSessionExpired();
      return handler.next(err);
    }

    try {
      final newToken = await store.read(StorageKeys.accessToken);
      final retryOptions = options
        ..extra[_retriedKey] = true
        ..headers['Authorization'] = 'Bearer $newToken';
      final response = await refreshDio.fetch<dynamic>(retryOptions);
      return handler.resolve(response);
    } on DioException catch (e) {
      return handler.next(e);
    }
  }

  /// Refresh the token pair, sharing a single in-flight future so concurrent
  /// callers don't issue multiple refreshes.
  Future<bool> _refresh() {
    return _inFlightRefresh ??=
        _doRefresh().whenComplete(() => _inFlightRefresh = null);
  }

  Future<bool> _doRefresh() async {
    final refreshToken = await store.read(StorageKeys.refreshToken);
    if (refreshToken == null || refreshToken.isEmpty) return false;
    try {
      final resp = await refreshDio.post<Map<String, dynamic>>(
        '/api/v1/auth/refresh',
        data: {'refresh_token': refreshToken},
      );
      final data = resp.data;
      if (data == null) return false;
      final access = data['access_token'] as String?;
      final refresh = data['refresh_token'] as String?;
      if (access == null || access.isEmpty) return false;
      await store.write(StorageKeys.accessToken, access);
      if (refresh != null && refresh.isNotEmpty) {
        await store.write(StorageKeys.refreshToken, refresh);
      }
      return true;
    } on DioException {
      return false;
    }
  }
}
