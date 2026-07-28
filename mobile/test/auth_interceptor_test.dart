import 'dart:convert';
import 'dart:typed_data';

import 'package:desksync_mobile/core/network/auth_interceptor.dart';
import 'package:dio/dio.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

/// Dio adapter that answers every request from a callback, so a test can stage
/// exactly how the backend behaves — including not answering at all.
class _ScriptedAdapter implements HttpClientAdapter {
  _ScriptedAdapter(this.respond);

  final Future<ResponseBody> Function(RequestOptions options) respond;

  @override
  Future<ResponseBody> fetch(
    RequestOptions options,
    Stream<Uint8List>? requestStream,
    Future<void>? cancelFuture,
  ) =>
      respond(options);

  @override
  void close({bool force = false}) {}
}

ResponseBody _json(int status, Object body) => ResponseBody.fromString(
      jsonEncode(body),
      status,
      headers: {
        Headers.contentTypeHeader: [Headers.jsonContentType],
      },
    );

void main() {
  late InMemorySecureStore store;
  late int expiries;

  setUp(() {
    store = InMemorySecureStore();
    expiries = 0;
  });

  /// A Dio with the interceptor installed, sharing one scripted adapter with the
  /// bare instance the interceptor uses for refreshes and replays.
  Dio client(Future<ResponseBody> Function(RequestOptions) respond) {
    final adapter = _ScriptedAdapter(respond);
    final refreshDio = Dio(BaseOptions(baseUrl: 'https://desksync.test'))
      ..httpClientAdapter = adapter;
    return Dio(BaseOptions(baseUrl: 'https://desksync.test'))
      ..httpClientAdapter = adapter
      ..interceptors.add(
        AuthInterceptor(
          store: store,
          refreshDio: refreshDio,
          onSessionExpired: () => expiries++,
        ),
      );
  }

  test('a rotated pair is stored and the original request replayed', () async {
    final dio = client((options) async {
      if (options.path.contains('/auth/refresh')) {
        return _json(200, {'access_token': 'a2', 'refresh_token': 'r2'});
      }
      if (options.headers['Authorization'] == 'Bearer a2') {
        return _json(200, {'devices': <String>[]});
      }
      return _json(401, {'error': 'token expired'});
    });
    await store.saveTokens(accessToken: 'a1', refreshToken: 'r1');

    final response = await dio.get<Map<String, dynamic>>('/api/v1/devices');

    expect(response.statusCode, 200);
    expect(await store.readAccessToken(), 'a2');
    expect(await store.readRefreshToken(), 'r2');
    expect(expiries, 0);
  });

  test('a refresh the backend refuses ends the session', () async {
    final dio = client((options) async => _json(401, {'error': 'nope'}));
    await store.saveTokens(accessToken: 'a1', refreshToken: 'r1');

    await expectLater(
      dio.get<Map<String, dynamic>>('/api/v1/devices'),
      throwsA(isA<DioException>()),
    );

    expect(await store.readRefreshToken(), isNull, reason: 'tokens are dead');
    expect(expiries, 1);
  });

  test('a refresh that never reaches the backend keeps the session', () async {
    // The bug this pins: treating every failed refresh as an expired session
    // signed the user out on any connectivity blip, even though the refresh
    // token stays valid for weeks.
    final dio = client((options) async {
      if (options.path.contains('/auth/refresh')) {
        throw DioException.connectionError(
          requestOptions: options,
          reason: 'no route to host',
        );
      }
      return _json(401, {'error': 'token expired'});
    });
    await store.saveTokens(accessToken: 'a1', refreshToken: 'r1');

    await expectLater(
      dio.get<Map<String, dynamic>>('/api/v1/devices'),
      throwsA(isA<DioException>()),
    );

    expect(await store.readRefreshToken(), 'r1');
    expect(await store.readAccessToken(), 'a1');
    expect(expiries, 0, reason: 'the user should not be asked to sign in again');
  });

  test('a backend fault during refresh keeps the session', () async {
    final dio = client((options) async {
      if (options.path.contains('/auth/refresh')) {
        return _json(503, {'error': 'upstream unavailable'});
      }
      return _json(401, {'error': 'token expired'});
    });
    await store.saveTokens(accessToken: 'a1', refreshToken: 'r1');

    await expectLater(
      dio.get<Map<String, dynamic>>('/api/v1/devices'),
      throwsA(isA<DioException>()),
    );

    expect(await store.readRefreshToken(), 'r1');
    expect(expiries, 0);
  });
}
