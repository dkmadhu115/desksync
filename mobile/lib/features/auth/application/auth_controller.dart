import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/network/api_exception.dart';
import '../../../core/network/dio_client.dart';
import '../data/auth_repository.dart';
import '../domain/auth_state.dart';

/// Orchestrates authentication: sign-in, registration, logout, launch
/// bootstrap, and reacting to expired sessions. UI observes [AuthState] and
/// the router redirects on status changes.
class AuthController extends Notifier<AuthState> {
  AuthRepository get _repo => ref.read(authRepositoryProvider);

  @override
  AuthState build() {
    // React to session expiry signalled by the Dio auth interceptor.
    ref.listen<int>(sessionExpiredProvider, (previous, next) {
      if (next > 0) onSessionExpired();
    });
    return const AuthState(status: AuthStatus.unknown);
  }

  /// Determine the initial auth status from persisted tokens. Called once on
  /// launch before the first frame settles.
  Future<void> bootstrap() async {
    final hasSession = await _repo.hasValidSession();
    state = AuthState(
      status: hasSession
          ? AuthStatus.authenticated
          : AuthStatus.unauthenticated,
    );
  }

  /// Sign in with email + password.
  Future<void> signInWithEmail(String email, String password) async {
    final trimmed = email.trim();
    if (!_isValidEmail(trimmed) || password.isEmpty) {
      state = state.copyWith(
        status: AuthStatus.unauthenticated,
        errorMessage: 'Enter a valid email and password.',
      );
      return;
    }
    await _run(() => _repo.login(trimmed, password), email: trimmed);
  }

  /// Register a new account.
  Future<void> register(
    String email,
    String password, {
    String? displayName,
  }) async {
    final trimmed = email.trim();
    if (!_isValidEmail(trimmed)) {
      state = state.copyWith(
        status: AuthStatus.unauthenticated,
        errorMessage: 'Enter a valid email address.',
      );
      return;
    }
    if (password.length < 12) {
      state = state.copyWith(
        status: AuthStatus.unauthenticated,
        errorMessage: 'Password must be at least 12 characters.',
      );
      return;
    }
    await _run(
      () => _repo.register(trimmed, password, displayName: displayName),
      email: trimmed,
    );
  }

  /// Sign the user out (revokes the refresh token, clears local state).
  Future<void> signOut() async {
    try {
      await _repo.logout();
    } finally {
      state = const AuthState(status: AuthStatus.unauthenticated);
    }
  }

  /// Handle a session that expired mid-use (refresh failed). Clears local
  /// tokens and returns to the unauthenticated state.
  void onSessionExpired() {
    if (state.status == AuthStatus.unauthenticated) return;
    _repo.clearLocalSession();
    state = const AuthState(
      status: AuthStatus.unauthenticated,
      errorMessage: 'Your session expired. Please sign in again.',
    );
  }

  Future<void> _run(
    Future<void> Function() action, {
    required String email,
  }) async {
    state = state.copyWith(status: AuthStatus.authenticating);
    try {
      await action();
      state = AuthState(status: AuthStatus.authenticated, userEmail: email);
    } on ApiException catch (e) {
      state = AuthState(
        status: AuthStatus.unauthenticated,
        errorMessage: e.message,
      );
    } catch (_) {
      state = const AuthState(
        status: AuthStatus.unauthenticated,
        errorMessage: 'Unexpected error. Please try again.',
      );
    }
  }

  static bool _isValidEmail(String email) {
    final re = RegExp(r'^[^@\s]+@[^@\s]+\.[^@\s]+$');
    return re.hasMatch(email);
  }
}

/// Provides the [AuthController] and its [AuthState].
final authControllerProvider =
    NotifierProvider<AuthController, AuthState>(AuthController.new);
