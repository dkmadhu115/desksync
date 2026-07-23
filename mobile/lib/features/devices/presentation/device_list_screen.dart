import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../app/router.dart';

/// Lists the user's paired devices with online/offline status. Phase 1 renders
/// a static placeholder; Phase 4/6 load real devices from the device service.
class DeviceListScreen extends ConsumerWidget {
  /// Creates the device list screen.
  const DeviceListScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('My Devices'),
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => context.go(Routes.pairing),
        icon: const Icon(Icons.add_link),
        label: const Text('Pair device'),
      ),
      body: const Center(
        child: Padding(
          padding: EdgeInsets.all(24),
          child: Text(
            'No paired devices yet.\nTap “Pair device” to connect your laptop.',
            textAlign: TextAlign.center,
          ),
        ),
      ),
    );
  }
}
