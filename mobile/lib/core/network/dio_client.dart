import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../config/env.dart';
import '../storage/secure_storage.dart';
import 'auth_interceptor.dart';

/// Signals that the current session has expired and the user must sign in
/// again. The auth controller listens to this to reset its state and the
/// router redirects to the login screen. The integer is a monotonically
/// increasing "tick" — listeners react to any change.
class SessionExpiredNotifier extends Notifier<int> {
  @override
  int build() => 0;

  /// Emit a session-expired signal.
  void trigger() => state++;
}

/// Provides the session-expiry signal.
final sessionExpiredProvider =
    NotifierProvider<SessionExpiredNotifier, int>(SessionExpiredNotifier.new);

BaseOptions _baseOptions() => BaseOptions(
      baseUrl: Env.apiBaseUrl,
      connectTimeout: const Duration(milliseconds: Env.requestTimeoutMs),
      receiveTimeout: const Duration(milliseconds: Env.requestTimeoutMs),
      sendTimeout: const Duration(milliseconds: Env.requestTimeoutMs),
      contentType: 'application/json',
      // We handle non-2xx ourselves by mapping to ApiException.
      validateStatus: (status) => status != null && status >= 200 && status < 300,
    );

/// Builds and provides the app-wide [Dio] HTTP client, pre-configured with the
/// gateway base URL, timeouts, and the [AuthInterceptor] that attaches the
/// bearer token and refreshes it on expiry. Certificate pinning is added in the
/// security-hardening phase (Phase 9).
final dioProvider = Provider<Dio>((ref) {
  final store = ref.watch(secureStoreProvider);

  final dio = Dio(_baseOptions());
  // A separate client with no auth interceptor, used for token refresh and for
  // replaying requests after a refresh (prevents recursive interception).
  final refreshDio = Dio(_baseOptions());

  dio.interceptors.add(
    AuthInterceptor(
      store: store,
      refreshDio: refreshDio,
      onSessionExpired: () {
        // Bump the counter; listeners react by clearing auth state.
        ref.read(sessionExpiredProvider.notifier).trigger();
      },
    ),
  );

  return dio;
});
