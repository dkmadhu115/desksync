import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../domain/auth_state.dart';

/// Manages authentication state for the app.
///
/// Phase 1 provides the state machine and a stubbed [signInWithEmail] that
/// performs input validation only. Phase 2/4 wire this to the auth service via
/// Dio and persist tokens in secure storage.
class AuthController extends Notifier<AuthState> {
  @override
  AuthState build() => const AuthState(status: AuthStatus.unauthenticated);

  /// Attempt an email/password sign-in. Currently validates input and
  /// transitions state; the network call is added in a later phase.
  Future<void> signInWithEmail(String email, String password) async {
    if (!_isValidEmail(email) || password.isEmpty) {
      state = state.copyWith(
        status: AuthStatus.unauthenticated,
        errorMessage: 'Enter a valid email and password.',
      );
      return;
    }

    state = state.copyWith(status: AuthStatus.authenticating);
    // Placeholder for the real auth-service request (Phase 2/4).
    state = AuthState(status: AuthStatus.authenticated, userEmail: email);
  }

  /// Sign the user out and clear state.
  void signOut() {
    state = const AuthState(status: AuthStatus.unauthenticated);
  }

  static bool _isValidEmail(String email) {
    final re = RegExp(r'^[^@\s]+@[^@\s]+\.[^@\s]+$');
    return re.hasMatch(email);
  }
}

/// Provides the [AuthController] and its [AuthState].
final authControllerProvider =
    NotifierProvider<AuthController, AuthState>(AuthController.new);
