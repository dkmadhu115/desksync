import 'package:desksync_mobile/core/network/api_exception.dart';
import 'package:desksync_mobile/core/storage/secure_storage.dart';
import 'package:desksync_mobile/features/auth/application/auth_controller.dart';
import 'package:desksync_mobile/features/auth/data/auth_api.dart';
import 'package:desksync_mobile/features/auth/domain/auth_state.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

void main() {
  late InMemorySecureStore store;
  late FakeAuthApi api;
  late ProviderContainer container;

  ProviderContainer makeContainer() => ProviderContainer(
        overrides: [
          secureStoreProvider.overrideWithValue(store),
          authApiProvider.overrideWithValue(api),
        ],
      );

  setUp(() {
    store = InMemorySecureStore();
    api = FakeAuthApi();
    container = makeContainer();
  });
  tearDown(() => container.dispose());

  test('starts in unknown state', () {
    expect(container.read(authControllerProvider).status, AuthStatus.unknown);
  });

  test('bootstrap without token resolves to unauthenticated', () async {
    await container.read(authControllerProvider.notifier).bootstrap();
    expect(
      container.read(authControllerProvider).status,
      AuthStatus.unauthenticated,
    );
  });

  test('bootstrap with a stored token resolves to authenticated', () async {
    await store.write(StorageKeys.accessToken, 'existing');
    await container.read(authControllerProvider.notifier).bootstrap();
    expect(
      container.read(authControllerProvider).status,
      AuthStatus.authenticated,
    );
  });

  test('rejects invalid credentials without calling the API', () async {
    await container
        .read(authControllerProvider.notifier)
        .signInWithEmail('not-an-email', '');
    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.unauthenticated);
    expect(state.errorMessage, isNotNull);
    expect(await store.readAccessToken(), isNull);
  });

  test('successful login authenticates and persists tokens', () async {
    await container
        .read(authControllerProvider.notifier)
        .signInWithEmail('dev@example.com', 'secret123');

    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.authenticated);
    expect(state.userEmail, 'dev@example.com');
    expect(await store.readAccessToken(), 'access-123');
    expect(await store.readRefreshToken(), 'refresh-123');
  });

  test('API failure surfaces a message and stays unauthenticated', () async {
    api.error = const ApiException(
      code: 'unauthorized',
      message: 'Invalid email or password.',
      statusCode: 401,
    );
    await container
        .read(authControllerProvider.notifier)
        .signInWithEmail('dev@example.com', 'wrongpass');

    final state = container.read(authControllerProvider);
    expect(state.status, AuthStatus.unauthenticated);
    expect(state.errorMessage, 'Invalid email or password.');
  });

  test('register enforces a minimum password length', () async {
    await container
        .read(authControllerProvider.notifier)
        .register('dev@example.com', 'short');
    expect(
      container.read(authControllerProvider).status,
      AuthStatus.unauthenticated,
    );
    expect(container.read(authControllerProvider).errorMessage, isNotNull);
  });

  test('signOut revokes and clears the session', () async {
    await container
        .read(authControllerProvider.notifier)
        .signInWithEmail('dev@example.com', 'secret123');
    await container.read(authControllerProvider.notifier).signOut();

    expect(
      container.read(authControllerProvider).status,
      AuthStatus.unauthenticated,
    );
    expect(api.logoutCalls, 1);
    expect(await store.readAccessToken(), isNull);
  });
}
