import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../app/router.dart';
import '../../devices/application/devices_controller.dart';
import '../application/pairing_controller.dart';
import '../domain/pairing_link.dart';
import 'qr_scan_screen.dart';

/// Device pairing screen. Offers QR scanning and manual-code confirmation, both
/// following the backend pairing contract.
class PairingScreen extends ConsumerStatefulWidget {
  /// Creates the pairing screen.
  const PairingScreen({super.key});

  @override
  ConsumerState<PairingScreen> createState() => _PairingScreenState();
}

class _PairingScreenState extends ConsumerState<PairingScreen> {
  final _formKey = GlobalKey<FormState>();
  final _pairingIdController = TextEditingController();
  final _codeController = TextEditingController();

  @override
  void dispose() {
    _pairingIdController.dispose();
    _codeController.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final state = ref.watch(pairingControllerProvider);

    // On success, refresh the device list and return to it.
    ref.listen<PairingUiState>(pairingControllerProvider, (previous, next) {
      if (next.phase == PairingPhase.success) {
        ref.read(devicesControllerProvider.notifier).refresh();
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text(next.message ?? 'Device paired.')),
        );
        ref.read(pairingControllerProvider.notifier).reset();
        context.go(Routes.devices);
      }
    });

    return Scaffold(
      appBar: AppBar(title: const Text('Pair a device')),
      body: ListView(
        padding: const EdgeInsets.all(24),
        children: [
          Card(
            child: ListTile(
              leading: const Icon(Icons.qr_code_scanner),
              title: const Text('Scan QR code'),
              subtitle:
                  const Text('Point your camera at the code on your laptop'),
              trailing: const Icon(Icons.chevron_right),
              onTap: state.isSubmitting ? null : _onScan,
            ),
          ),
          const SizedBox(height: 24),
          Text(
            'Enter pairing details',
            style: Theme.of(context).textTheme.titleMedium,
          ),
          const SizedBox(height: 4),
          Text(
            'Your laptop shows a pairing id and an 8-digit code.',
            style: Theme.of(context).textTheme.bodySmall,
          ),
          const SizedBox(height: 16),
          Form(
            key: _formKey,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                TextFormField(
                  controller: _pairingIdController,
                  enabled: !state.isSubmitting,
                  decoration: const InputDecoration(
                    labelText: 'Pairing ID',
                    prefixIcon: Icon(Icons.link),
                    border: OutlineInputBorder(),
                  ),
                  validator: (v) =>
                      (v ?? '').trim().isEmpty ? 'Pairing ID is required' : null,
                ),
                const SizedBox(height: 16),
                TextFormField(
                  controller: _codeController,
                  enabled: !state.isSubmitting,
                  keyboardType: TextInputType.number,
                  maxLength: 8,
                  inputFormatters: [
                    FilteringTextInputFormatter.digitsOnly,
                  ],
                  decoration: const InputDecoration(
                    labelText: '8-digit code',
                    prefixIcon: Icon(Icons.dialpad),
                    border: OutlineInputBorder(),
                    counterText: '',
                  ),
                  validator: (v) {
                    final value = (v ?? '').trim();
                    if (value.length != 8) return 'Enter the 8-digit code';
                    return null;
                  },
                ),
                if (state.phase == PairingPhase.error &&
                    state.message != null) ...[
                  const SizedBox(height: 12),
                  Text(
                    state.message!,
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.error,
                    ),
                  ),
                ],
                const SizedBox(height: 24),
                FilledButton(
                  onPressed: state.isSubmitting ? null : _onConfirm,
                  child: state.isSubmitting
                      ? const SizedBox(
                          height: 20,
                          width: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : const Text('Pair device'),
                ),
              ],
            ),
          ),
        ],
      ),
    );
  }

  void _onConfirm() {
    if (!(_formKey.currentState?.validate() ?? false)) return;
    FocusScope.of(context).unfocus();
    ref.read(pairingControllerProvider.notifier).confirm(
          pairingId: _pairingIdController.text,
          code: _codeController.text,
        );
  }

  Future<void> _onScan() async {
    final link = await Navigator.of(context).push<PairingLink>(
      MaterialPageRoute(builder: (_) => const QrScanScreen()),
    );
    if (link == null || !mounted) return;
    // Reflect the scanned values in the form, then confirm immediately.
    _pairingIdController.text = link.pairingId;
    _codeController.text = link.code;
    ref.read(pairingControllerProvider.notifier).confirm(
          pairingId: link.pairingId,
          code: link.code,
        );
  }
}
