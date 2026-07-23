import 'package:dio/dio.dart';

/// A typed, user-presentable error raised by the data layer.
///
/// Dio surfaces low-level [DioException]s that are awkward to show to users and
/// to assert on in tests. [ApiException] normalizes them: it maps transport
/// failures (timeouts, no connectivity) and the backend's structured error body
/// (`{ "error": ..., "message": ..., "request_id": ... }`, per the OpenAPI
/// contract) into a single shape with a stable [code] and a human [message].
class ApiException implements Exception {
  /// Creates an API exception.
  const ApiException({
    required this.code,
    required this.message,
    this.statusCode,
    this.requestId,
  });

  /// Stable, machine-readable error code (e.g. `unauthorized`, `network`,
  /// `timeout`, `rate_limited`, or a backend-provided `error` value).
  final String code;

  /// Human-readable message suitable for display.
  final String message;

  /// HTTP status code when the failure came from a response.
  final int? statusCode;

  /// Correlation id echoed by the backend, useful for support/debugging.
  final String? requestId;

  /// Whether this represents an authentication failure (expired/invalid creds).
  bool get isUnauthorized => statusCode == 401;

  /// Whether the caller was rate limited.
  bool get isRateLimited => statusCode == 429;

  /// Build an [ApiException] from a Dio error, decoding the backend error body
  /// when present.
  factory ApiException.fromDio(DioException e) {
    switch (e.type) {
      case DioExceptionType.connectionTimeout:
      case DioExceptionType.sendTimeout:
      case DioExceptionType.receiveTimeout:
      case DioExceptionType.transformTimeout:
        return const ApiException(
          code: 'timeout',
          message: 'The server took too long to respond. Please try again.',
        );
      case DioExceptionType.connectionError:
        return const ApiException(
          code: 'network',
          message: 'Cannot reach the server. Check your connection.',
        );
      case DioExceptionType.cancel:
        return const ApiException(
          code: 'cancelled',
          message: 'The request was cancelled.',
        );
      case DioExceptionType.badCertificate:
        return const ApiException(
          code: 'bad_certificate',
          message: 'The server certificate could not be verified.',
        );
      case DioExceptionType.badResponse:
      case DioExceptionType.unknown:
        return ApiException.fromResponse(e.response, fallback: e.message);
    }
  }

  /// Build an [ApiException] from an HTTP [response], decoding the standard
  /// backend error envelope when available.
  factory ApiException.fromResponse(Response<dynamic>? response,
      {String? fallback}) {
    final status = response?.statusCode;
    final data = response?.data;

    String code = _defaultCodeForStatus(status);
    String message = _defaultMessageForStatus(status, fallback);
    String? requestId;

    if (data is Map) {
      final err = data['error'];
      final msg = data['message'];
      final rid = data['request_id'];
      if (err is String && err.isNotEmpty) code = err;
      if (msg is String && msg.isNotEmpty) message = msg;
      if (rid is String && rid.isNotEmpty) requestId = rid;
    }

    return ApiException(
      code: code,
      message: message,
      statusCode: status,
      requestId: requestId,
    );
  }

  static String _defaultCodeForStatus(int? status) {
    switch (status) {
      case 400:
        return 'invalid_input';
      case 401:
        return 'unauthorized';
      case 403:
        return 'forbidden';
      case 404:
        return 'not_found';
      case 409:
        return 'conflict';
      case 429:
        return 'rate_limited';
      default:
        return 'server_error';
    }
  }

  static String _defaultMessageForStatus(int? status, String? fallback) {
    switch (status) {
      case 400:
        return 'The request was invalid.';
      case 401:
        return 'Your session has expired. Please sign in again.';
      case 403:
        return 'You do not have permission to do that.';
      case 404:
        return 'The requested resource was not found.';
      case 409:
        return 'That resource already exists.';
      case 429:
        return 'Too many requests. Please slow down and try again.';
      default:
        return fallback?.isNotEmpty == true
            ? fallback!
            : 'Something went wrong. Please try again.';
    }
  }

  @override
  String toString() => 'ApiException($code, status: $statusCode): $message';
}
