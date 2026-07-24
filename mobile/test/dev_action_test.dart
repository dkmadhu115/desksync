import 'package:desksync_mobile/features/devtools/domain/dev_action.dart';
import 'package:desksync_mobile/features/devtools/domain/dev_catalog.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  group('DevActionRequest.toJson', () {
    test('launch editor flattens kind with snake_case wire values', () {
      final req = DevActionRequest(
        requestId: 'r1',
        action: const LaunchEditorAction(DevEditor.vsCode, workspaceId: 'ws1'),
      );
      expect(req.toJson(), {
        'request_id': 'r1',
        'action': 'launch_editor',
        'editor': 'vs_code',
        'workspace_id': 'ws1',
      });
    });

    test('omits workspace_id when null', () {
      final req = DevActionRequest(
        requestId: 'r2',
        action: const OpenTerminalAction(DevTerminal.appleTerminal),
      );
      final json = req.toJson();
      expect(json['action'], 'open_terminal');
      expect(json['terminal'], 'apple_terminal');
      expect(json.containsKey('workspace_id'), isFalse);
    });

    test('run shortcut carries tool + shortcut id', () {
      final req = DevActionRequest(
        requestId: 'r3',
        action: const RunShortcutAction(DevTool.git, 'status', workspaceId: 'ws'),
      );
      expect(req.toJson(), {
        'request_id': 'r3',
        'action': 'run_shortcut',
        'tool': 'git',
        'shortcut_id': 'status',
        'workspace_id': 'ws',
      });
    });

    test('ssh connect carries host id and terminal', () {
      final req = DevActionRequest(
        requestId: 'r4',
        action: const SshConnectAction('prod', DevTerminal.iTerm),
      );
      expect(req.toJson(), {
        'request_id': 'r4',
        'action': 'ssh_connect',
        'host_id': 'prod',
        'terminal': 'i_term',
      });
    });
  });

  group('catalog', () {
    test('git shortcuts all require a workspace', () {
      final git = shortcutsForTool(DevTool.git);
      expect(git, isNotEmpty);
      expect(git.every((s) => s.needsWorkspace), isTrue);
    });

    test('global docker shortcuts do not require a workspace', () {
      final ps = shortcutsForTool(DevTool.docker).firstWhere((s) => s.id == 'ps');
      expect(ps.needsWorkspace, isFalse);
    });
  });
}
