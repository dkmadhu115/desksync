/// High-level authentication status for the app.
enum AuthStatus {
  /// Initial/unknown state before the launch bootstrap has run.
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

  /// Whether an auth request is currently in flight.
  bool get isBusy => status == AuthStatus.authenticating;

  /// Returns a copy with the given fields replaced. [errorMessage] is always
  /// replaced (pass null to clear it) so stale errors don't linger.
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
