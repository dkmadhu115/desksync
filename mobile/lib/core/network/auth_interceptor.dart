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
/// - Only a refresh the backend actually *refuses* ends the session. A refresh
///   that could not be delivered (no connectivity, timeout, server error) leaves
///   the stored tokens alone: a refresh token is valid for weeks, so throwing it
///   away over a dropped request would sign the user out every time they walked
///   through a tunnel.
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

  Future<_RefreshOutcome>? _inFlightRefresh;

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

    final outcome =
        tokenAlreadyRotated ? _RefreshOutcome.renewed : await _refresh();
    switch (outcome) {
      case _RefreshOutcome.refused:
        await store.delete(StorageKeys.accessToken);
        await store.delete(StorageKeys.refreshToken);
        onSessionExpired();
        return handler.next(err);
      case _RefreshOutcome.undelivered:
        // The credentials are probably fine; this request is not. Fail it and
        // let the next one try again with the session intact.
        return handler.next(err);
      case _RefreshOutcome.renewed:
        break;
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
  Future<_RefreshOutcome> _refresh() {
    return _inFlightRefresh ??=
        _doRefresh().whenComplete(() => _inFlightRefresh = null);
  }

  Future<_RefreshOutcome> _doRefresh() async {
    final refreshToken = await store.read(StorageKeys.refreshToken);
    if (refreshToken == null || refreshToken.isEmpty) {
      return _RefreshOutcome.refused;
    }
    try {
      final resp = await refreshDio.post<Map<String, dynamic>>(
        '/api/v1/auth/refresh',
        data: {'refresh_token': refreshToken},
      );
      final access = resp.data?['access_token'] as String?;
      final refresh = resp.data?['refresh_token'] as String?;
      if (access == null || access.isEmpty) {
        // A 2xx without a token is a backend fault, not a dead session.
        return _RefreshOutcome.undelivered;
      }
      await store.write(StorageKeys.accessToken, access);
      if (refresh != null && refresh.isNotEmpty) {
        await store.write(StorageKeys.refreshToken, refresh);
      }
      return _RefreshOutcome.renewed;
    } on DioException catch (e) {
      final status = e.response?.statusCode;
      // Only the backend rejecting the token means the user must sign in again.
      // Anything else — offline, timeout, 5xx, proxy error — is temporary.
      final rejected = status == 400 || status == 401 || status == 403;
      return rejected
          ? _RefreshOutcome.refused
          : _RefreshOutcome.undelivered;
    }
  }
}

/// What came of a refresh attempt.
enum _RefreshOutcome {
  /// A new pair was stored.
  renewed,

  /// The backend rejected the refresh token: the user must sign in again.
  refused,

  /// The attempt never got an answer, so the session is still presumed good.
  undelivered,
}
