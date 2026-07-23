import 'package:desksync_mobile/app/app.dart';
import 'package:desksync_mobile/core/storage/secure_storage.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

void main() {
  testWidgets('boots through splash to the login screen when signed out',
      (tester) async {
    await tester.pumpWidget(
      ProviderScope(
        overrides: [
          // Use in-memory storage so no session exists and no keychain is hit.
          secureStoreProvider.overrideWithValue(InMemorySecureStore()),
        ],
        child: const DeskSyncApp(),
      ),
    );

    // Splash shows first while the bootstrap runs.
    expect(find.byType(CircularProgressIndicator), findsOneWidget);

    // After bootstrap resolves (no token), the router redirects to login.
    await tester.pumpAndSettle();

    expect(find.text('DeskSync'), findsOneWidget);
    expect(find.text('Sign in'), findsOneWidget);
    expect(find.byType(TextFormField), findsNWidgets(2));
  });
}
