import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../application/control_sink.dart';
import '../application/dev_action_controller.dart';
import '../domain/dev_action.dart';
import '../domain/dev_catalog.dart';

/// Quick Launch: trigger developer actions on the connected desktop — launch
/// editors/terminals, run curated tool shortcuts, and SSH into saved hosts.
///
/// Actions flow over the active session's control data channel. Workspaces and
/// SSH hosts are referenced by the ids configured on the agent (never raw
/// paths), matching the agent's allowlist model.
class QuickLaunchScreen extends ConsumerStatefulWidget {
  /// Creates the screen.
  const QuickLaunchScreen({super.key});

  @override
  ConsumerState<QuickLaunchScreen> createState() => _QuickLaunchScreenState();
}

class _QuickLaunchScreenState extends ConsumerState<QuickLaunchScreen> {
  final _workspaceId = TextEditingController();
  final _sshHostId = TextEditingController();
  DevTerminal _sshTerminal = DevTerminal.appleTerminal;

  @override
  void dispose() {
    _workspaceId.dispose();
    _sshHostId.dispose();
    super.dispose();
  }

  DevActionController get _actions =>
      ref.read(devActionControllerProvider.notifier);

  String? get _workspaceOrNull {
    final v = _workspaceId.text.trim();
    return v.isEmpty ? null : v;
  }

  void _toast(String message) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(content: Text(message)));
  }

  @override
  Widget build(BuildContext context) {
    final connected = ref.watch(switchableControlSinkProvider).hasTarget;
    final sent = ref.watch(devActionControllerProvider);

    return Scaffold(
      appBar: AppBar(
        title: const Text('Quick Launch'),
        actions: [
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Center(child: Text('$sent sent')),
          ),
        ],
      ),
      body: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          if (!connected)
            const Card(
              color: Color(0xFF3A2E00),
              child: ListTile(
                leading: Icon(Icons.info_outline),
                title: Text('Not connected'),
                subtitle: Text(
                  'Open a desktop session first; actions are sent over that '
                  'connection.',
                ),
              ),
            ),
          const SizedBox(height: 8),
          TextField(
            controller: _workspaceId,
            decoration: const InputDecoration(
              labelText: 'Workspace id (optional)',
              helperText: 'A workspace saved on your desktop; leave blank for none',
              border: OutlineInputBorder(),
              isDense: true,
            ),
          ),
          const SizedBox(height: 24),
          _sectionTitle('Editors'),
          Wrap(
            spacing: 8,
            children: [
              for (final e in DevEditor.values)
                ActionChip(
                  avatar: const Icon(Icons.code, size: 18),
                  label: Text(e.label),
                  onPressed: () {
                    _actions.launchEditor(e, workspaceId: _workspaceOrNull);
                    _toast('Launching ${e.label}');
                  },
                ),
            ],
          ),
          const SizedBox(height: 24),
          _sectionTitle('Terminals'),
          Wrap(
            spacing: 8,
            children: [
              for (final t in DevTerminal.values)
                ActionChip(
                  avatar: const Icon(Icons.terminal, size: 18),
                  label: Text(t.label),
                  onPressed: () {
                    _actions.openTerminal(t, workspaceId: _workspaceOrNull);
                    _toast('Opening ${t.label}');
                  },
                ),
            ],
          ),
          const SizedBox(height: 24),
          _sectionTitle('Tool shortcuts'),
          for (final tool in DevTool.values) _toolSection(tool),
          const SizedBox(height: 24),
          _sectionTitle('SSH'),
          TextField(
            controller: _sshHostId,
            decoration: const InputDecoration(
              labelText: 'SSH host id',
              helperText: 'An SSH host saved on your desktop',
              border: OutlineInputBorder(),
              isDense: true,
            ),
          ),
          const SizedBox(height: 12),
          Row(
            children: [
              Expanded(
                child: DropdownButtonFormField<DevTerminal>(
                  initialValue: _sshTerminal,
                  decoration: const InputDecoration(
                    labelText: 'In terminal',
                    border: OutlineInputBorder(),
                    isDense: true,
                  ),
                  items: [
                    for (final t in DevTerminal.values)
                      DropdownMenuItem(value: t, child: Text(t.label)),
                  ],
                  onChanged: (v) =>
                      setState(() => _sshTerminal = v ?? _sshTerminal),
                ),
              ),
              const SizedBox(width: 12),
              FilledButton.icon(
                icon: const Icon(Icons.login),
                label: const Text('Connect'),
                onPressed: _connectSsh,
              ),
            ],
          ),
        ],
      ),
    );
  }

  Widget _sectionTitle(String text) => Padding(
        padding: const EdgeInsets.only(bottom: 8),
        child: Text(text, style: const TextStyle(fontWeight: FontWeight.bold)),
      );

  Widget _toolSection(DevTool tool) {
    final shortcuts = shortcutsForTool(tool);
    return Card(
      child: ExpansionTile(
        title: Text(tool.label),
        childrenPadding: EdgeInsets.zero,
        children: [
          for (final s in shortcuts)
            ListTile(
              dense: true,
              title: Text(s.id),
              subtitle: Text(s.description),
              trailing: s.needsWorkspace && _workspaceOrNull == null
                  ? const Tooltip(
                      message: 'Requires a workspace id',
                      child: Icon(Icons.folder_off_outlined),
                    )
                  : const Icon(Icons.play_arrow),
              onTap: s.needsWorkspace && _workspaceOrNull == null
                  ? null
                  : () {
                      _actions.runShortcut(
                        tool,
                        s.id,
                        workspaceId: _workspaceOrNull,
                      );
                      _toast('Running ${tool.label} ${s.id}');
                    },
            ),
        ],
      ),
    );
  }

  void _connectSsh() {
    final hostId = _sshHostId.text.trim();
    if (hostId.isEmpty) {
      _toast('Enter an SSH host id first.');
      return;
    }
    _actions.sshConnect(hostId, _sshTerminal);
    _toast('Connecting to $hostId');
  }
}
