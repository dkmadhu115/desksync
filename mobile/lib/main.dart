import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import 'app/app.dart';

/// Entry point for the DeskSync mobile client.
///
/// The whole widget tree is wrapped in a Riverpod [ProviderScope] so that
/// controllers and services can be provided and overridden (e.g. in tests).
void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(const ProviderScope(child: DeskSyncApp()));
}
