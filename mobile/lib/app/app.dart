import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'router.dart';
import 'theme.dart';

/// Root application widget. It wires the router and theming and follows the
/// system light/dark preference.
class DeskSyncApp extends ConsumerWidget {
  /// Creates the root app widget.
  const DeskSyncApp({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final router = ref.watch(routerProvider);
    return MaterialApp.router(
      title: 'DeskSync',
      debugShowCheckedModeBanner: false,
      theme: DeskSyncTheme.light(),
      darkTheme: DeskSyncTheme.dark(),
      themeMode: ThemeMode.system,
      routerConfig: router,
    );
  }
}
