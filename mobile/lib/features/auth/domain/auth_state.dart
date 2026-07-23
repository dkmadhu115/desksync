/// High-level authentication status for the app.
enum AuthStatus {
  /// Initial/unknown state before any check has run.
  unknown,

  /// The user is not signed in.
  unauthenticated,

  /// An authentication request is in flight.
  authenticating,

  /// The user is signed in.
  authenticated,
}

/// Immutable authentication state exposed to the UI.
class AuthState {
  /// Creates an [AuthState].
  const AuthState({
    this.status = AuthStatus.unknown,
    this.userEmail,
    this.errorMessage,
  });

  /// The current status.
  final AuthStatus status;

  /// The signed-in user's email, when available.
  final String? userEmail;

  /// The last error message, when a request failed.
  final String? errorMessage;

  /// Whether the user is authenticated.
  bool get isAuthenticated => status == AuthStatus.authenticated;

  /// Returns a copy with the given fields replaced.
  AuthState copyWith({
    AuthStatus? status,
    String? userEmail,
    String? errorMessage,
  }) {
    return AuthState(
      status: status ?? this.status,
      userEmail: userEmail ?? this.userEmail,
      errorMessage: errorMessage,
    );
  }
}
