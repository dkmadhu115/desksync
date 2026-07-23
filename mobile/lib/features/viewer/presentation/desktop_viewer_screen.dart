import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// The remote desktop viewer. Phase 1 renders the scaffold and a placeholder
/// surface; Phase 5/7 render the live WebRTC video track and forward touch,
/// keyboard, and gesture input to the desktop agent.
class DesktopViewerScreen extends ConsumerWidget {
  /// Creates the viewer for the given [deviceId].
  const DesktopViewerScreen({required this.deviceId, super.key});

  /// The device being controlled.
  final String deviceId;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        title: Text('Device $deviceId'),
        actions: [
          IconButton(
            tooltip: 'Disconnect',
            icon: const Icon(Icons.close),
            onPressed: () {}, // Implemented in Phase 7.
          ),
        ],
      ),
      body: const Center(
        child: Text(
          'Remote stream will render here',
          style: TextStyle(color: Colors.white70),
        ),
      ),
    );
  }
}
