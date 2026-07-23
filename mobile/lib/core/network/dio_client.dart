import 'package:dio/dio.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../config/env.dart';
import '../storage/secure_storage.dart';

/// Builds and provides the app-wide [Dio] HTTP client, pre-configured with the
/// gateway base URL, timeouts, and an interceptor that attaches the bearer
/// access token. Certificate pinning is added in the security-hardening phase.
final dioProvider = Provider<Dio>((ref) {
  final store = ref.watch(secureStoreProvider);

  final dio = Dio(
    BaseOptions(
      baseUrl: Env.apiBaseUrl,
      connectTimeout: const Duration(milliseconds: Env.requestTimeoutMs),
      receiveTimeout: const Duration(milliseconds: Env.requestTimeoutMs),
      contentType: 'application/json',
    ),
  );

  dio.interceptors.add(
    InterceptorsWrapper(
      onRequest: (options, handler) async {
        final token = await store.read(StorageKeys.accessToken);
        if (token != null && token.isNotEmpty) {
          options.headers['Authorization'] = 'Bearer $token';
        }
        handler.next(options);
      },
    ),
  );

  return dio;
});
