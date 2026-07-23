import 'package:desksync_mobile/features/auth/application/auth_controller.dart';
import 'package:desksync_mobile/features/auth/domain/auth_state.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  late ProviderContainer container;

  setUp(() => container = ProviderContainer());
  tearDown(() => container.dispose());

  test('starts unauthenticated', () {
    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.unauthenticated);
    expect(state.isAuthenticated, isFalse);
  });

  test('rejects invalid credentials without authenticating', () async {
    final controller = container.read(authControllerProvider.notifier);
    await controller.signInWithEmail('not-an-email', '');

    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.unauthenticated);
    expect(state.errorMessage, isNotNull);
  });

  test('authenticates with valid credentials', () async {
    final controller = container.read(authControllerProvider.notifier);
    await controller.signInWithEmail('dev@example.com', 'secret123');

    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.authenticated);
    expect(state.userEmail, 'dev@example.com');
  });

  test('signOut returns to unauthenticated', () async {
    final controller = container.read(authControllerProvider.notifier);
    await controller.signInWithEmail('dev@example.com', 'secret123');
    controller.signOut();

    expect(container.read(authControllerProvider).status,
        AuthStatus.unauthenticated);
  });
}
