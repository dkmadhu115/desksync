import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../features/auth/application/auth_controller.dart';
import 'router.dart';
import 'theme.dart';

/// Root application widget. It wires the router and theming, follows the system
/// light/dark preference, and kicks off the one-time auth bootstrap that
/// restores a persisted session on launch.
class DeskSyncApp extends ConsumerStatefulWidget {
  /// Creates the root app widget.
  const DeskSyncApp({super.key});

  @override
  ConsumerState<DeskSyncApp> createState() => _DeskSyncAppState();
}

class _DeskSyncAppState extends ConsumerState<DeskSyncApp> {
  @override
  void initState() {
    super.initState();
    // Restore any persisted session; the router shows the splash until this
    // resolves the auth status.
    Future.microtask(
      () => ref.read(authControllerProvider.notifier).bootstrap(),
    );
  }

  @override
  Widget build(BuildContext context) {
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
