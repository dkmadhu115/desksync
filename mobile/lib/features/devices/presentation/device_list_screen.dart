import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../app/router.dart';
import '../../../core/network/api_exception.dart';
import '../../auth/application/auth_controller.dart';
import '../application/devices_controller.dart';
import '../domain/device.dart';

/// Lists the user's devices with presence, and lets the user open a controllable
/// desktop, pair a new device, revoke a device, or sign out.
class DeviceListScreen extends ConsumerWidget {
  /// Creates the device list screen.
  const DeviceListScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final devices = ref.watch(devicesControllerProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('My Devices'),
        actions: [
          IconButton(
            tooltip: 'Refresh',
            icon: const Icon(Icons.refresh),
            onPressed: () =>
                ref.read(devicesControllerProvider.notifier).refresh(),
          ),
          IconButton(
            tooltip: 'Sign out',
            icon: const Icon(Icons.logout),
            onPressed: () => ref.read(authControllerProvider.notifier).signOut(),
          ),
        ],
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: () => context.go(Routes.pairing),
        icon: const Icon(Icons.add_link),
        label: const Text('Pair device'),
      ),
      body: devices.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (err, _) => _ErrorView(
          message: err is ApiException
              ? err.message
              : 'Failed to load devices.',
          onRetry: () => ref.read(devicesControllerProvider.notifier).refresh(),
        ),
        data: (items) => _DeviceList(devices: items),
      ),
    );
  }
}

class _DeviceList extends ConsumerWidget {
  const _DeviceList({required this.devices});
  final List<Device> devices;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    return RefreshIndicator(
      onRefresh: () => ref.read(devicesControllerProvider.notifier).refresh(),
      child: devices.isEmpty
          ? ListView(
              // A scrollable is required for RefreshIndicator to work on empty.
              children: const [
                SizedBox(height: 120),
                _EmptyState(),
              ],
            )
          : ListView.separated(
              padding: const EdgeInsets.symmetric(vertical: 8),
              itemCount: devices.length,
              separatorBuilder: (_, _) => const Divider(height: 1),
              itemBuilder: (context, i) => _DeviceTile(device: devices[i]),
            ),
    );
  }
}

class _DeviceTile extends ConsumerWidget {
  const _DeviceTile({required this.device});
  final Device device;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final online = device.status == DeviceStatus.online;
    final scheme = Theme.of(context).colorScheme;

    return ListTile(
      leading: Icon(
        device.kind == DeviceKind.desktop
            ? Icons.desktop_mac_outlined
            : Icons.smartphone_outlined,
      ),
      title: Text(device.name),
      subtitle: Text(
        '${device.platform} • ${online ? 'Online' : 'Offline'}',
      ),
      trailing: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(Icons.circle, size: 10, color: online ? Colors.green : scheme.outline),
          const SizedBox(width: 12),
          PopupMenuButton<String>(
            onSelected: (value) {
              if (value == 'revoke') _confirmRevoke(context, ref);
            },
            itemBuilder: (context) => const [
              PopupMenuItem(value: 'revoke', child: Text('Revoke device')),
            ],
          ),
        ],
      ),
      onTap: device.isControllable
          ? () => context.go(Routes.viewerPath(device.id))
          : null,
      enabled: device.kind != DeviceKind.unknown,
    );
  }

  Future<void> _confirmRevoke(BuildContext context, WidgetRef ref) async {
    // Capture the messenger before any await so we don't use `context` across
    // an async gap.
    final messenger = ScaffoldMessenger.of(context);
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Revoke device?'),
        content: Text(
          'This removes "${device.name}" and revokes its access. '
          'You will need to pair it again.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Revoke'),
          ),
        ],
      ),
    );
    if (confirmed != true) return;

    try {
      await ref.read(devicesControllerProvider.notifier).remove(device.id);
      messenger.showSnackBar(
        SnackBar(content: Text('${device.name} revoked')),
      );
    } on ApiException catch (e) {
      messenger.showSnackBar(SnackBar(content: Text(e.message)));
    }
  }
}

class _EmptyState extends StatelessWidget {
  const _EmptyState();

  @override
  Widget build(BuildContext context) {
    return const Center(
      child: Padding(
        padding: EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(Icons.devices_other, size: 56),
            SizedBox(height: 12),
            Text(
              'No paired devices yet.\nTap “Pair device” to connect your laptop.',
              textAlign: TextAlign.center,
            ),
          ],
        ),
      ),
    );
  }
}

class _ErrorView extends StatelessWidget {
  const _ErrorView({required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            const Icon(Icons.cloud_off, size: 56),
            const SizedBox(height: 12),
            Text(message, textAlign: TextAlign.center),
            const SizedBox(height: 16),
            FilledButton.icon(
              onPressed: onRetry,
              icon: const Icon(Icons.refresh),
              label: const Text('Retry'),
            ),
          ],
        ),
      ),
    );
  }
}
