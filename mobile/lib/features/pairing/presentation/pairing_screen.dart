import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

/// Device pairing screen. Phase 1 shows the two pairing modes (QR scan and
/// manual code); the actual pairing handshake is implemented in Phase 6.
class PairingScreen extends ConsumerWidget {
  /// Creates the pairing screen.
  const PairingScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return Scaffold(
      appBar: AppBar(title: const Text('Pair a device')),
      body: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            Card(
              child: ListTile(
                leading: const Icon(Icons.qr_code_scanner),
                title: const Text('Scan QR code'),
                subtitle: const Text('Scan the code shown by the desktop agent'),
                onTap: () {}, // Implemented in Phase 6.
              ),
            ),
            const SizedBox(height: 12),
            Card(
              child: ListTile(
                leading: const Icon(Icons.dialpad),
                title: const Text('Enter pairing code'),
                subtitle: const Text('Type the 8-digit code from your laptop'),
                onTap: () {}, // Implemented in Phase 6.
              ),
            ),
          ],
        ),
      ),
    );
  }
}
