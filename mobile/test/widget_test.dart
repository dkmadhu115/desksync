import 'package:desksync_mobile/app/app.dart';
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  testWidgets('app boots to the login screen', (tester) async {
    await tester.pumpWidget(const ProviderScope(child: DeskSyncApp()));
    await tester.pumpAndSettle();

    // The login screen shows the app title and a sign-in button.
    expect(find.text('DeskSync'), findsOneWidget);
    expect(find.text('Sign in'), findsOneWidget);
    expect(find.byType(TextField), findsNWidgets(2));
  });
}
