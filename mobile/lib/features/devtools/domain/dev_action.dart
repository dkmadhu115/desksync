/// Client model of developer quick-launch actions.
///
/// The JSON produced here MUST match the Rust agent's `DevActionRequest`
/// (`desktop-agent/crates/devtools`): a flattened object with a snake_case
/// `action` discriminator, snake_case enum values, and id-only references to
/// workspaces/hosts (never raw paths or commands).
library;

/// GUI editors that can be launched.
enum DevEditor {
  /// Visual Studio Code.
  vsCode('vs_code', 'VS Code'),

  /// Cursor.
  cursor('cursor', 'Cursor'),

  /// Claude Desktop.
  claude('claude', 'Claude');

  const DevEditor(this.wire, this.label);

  /// The wire value matching the agent enum.
  final String wire;

  /// Human-friendly label for the UI.
  final String label;
}

/// Terminal emulators that can be launched.
enum DevTerminal {
  /// Warp.
  warp('warp', 'Warp'),

  /// macOS Terminal.app.
  appleTerminal('apple_terminal', 'Terminal'),

  /// iTerm2.
  iTerm('i_term', 'iTerm'),

  /// PowerShell.
  powerShell('power_shell', 'PowerShell'),

  /// Windows Terminal.
  windowsTerminal('windows_terminal', 'Windows Terminal');

  const DevTerminal(this.wire, this.label);

  /// The wire value matching the agent enum.
  final String wire;

  /// Human-friendly label for the UI.
  final String label;
}

/// Developer CLIs exposing curated shortcuts.
enum DevTool {
  /// Git.
  git('git', 'Git'),

  /// Docker.
  docker('docker', 'Docker'),

  /// Kubernetes CLI.
  kubectl('kubectl', 'kubectl'),

  /// Helm.
  helm('helm', 'Helm');

  const DevTool(this.wire, this.label);

  /// The wire value matching the agent enum.
  final String wire;

  /// Human-friendly label for the UI.
  final String label;
}

/// A developer action. Subtypes serialize their fields alongside an `action`
/// discriminator (the agent flattens the kind into the request object).
sealed class DevAction {
  const DevAction();

  /// The `action` discriminator value.
  String get action;

  /// The action-specific fields (merged into the request JSON).
  Map<String, dynamic> fields();
}

/// Launch an editor, optionally opening a registered workspace.
class LaunchEditorAction extends DevAction {
  /// Creates the action.
  const LaunchEditorAction(this.editor, {this.workspaceId});

  /// Editor to launch.
  final DevEditor editor;

  /// Optional registered workspace id.
  final String? workspaceId;

  @override
  String get action => 'launch_editor';

  @override
  Map<String, dynamic> fields() => {
        'editor': editor.wire,
        if (workspaceId != null) 'workspace_id': workspaceId,
      };
}

/// Open a terminal, optionally in a registered workspace.
class OpenTerminalAction extends DevAction {
  /// Creates the action.
  const OpenTerminalAction(this.terminal, {this.workspaceId});

  /// Terminal to open.
  final DevTerminal terminal;

  /// Optional registered workspace id.
  final String? workspaceId;

  @override
  String get action => 'open_terminal';

  @override
  Map<String, dynamic> fields() => {
        'terminal': terminal.wire,
        if (workspaceId != null) 'workspace_id': workspaceId,
      };
}

/// Run a built-in tool shortcut, optionally in a registered workspace.
class RunShortcutAction extends DevAction {
  /// Creates the action.
  const RunShortcutAction(this.tool, this.shortcutId, {this.workspaceId});

  /// Tool the shortcut belongs to.
  final DevTool tool;

  /// Shortcut id from the catalog.
  final String shortcutId;

  /// Optional registered workspace id.
  final String? workspaceId;

  @override
  String get action => 'run_shortcut';

  @override
  Map<String, dynamic> fields() => {
        'tool': tool.wire,
        'shortcut_id': shortcutId,
        if (workspaceId != null) 'workspace_id': workspaceId,
      };
}

/// Open an SSH session to a registered host in a terminal.
class SshConnectAction extends DevAction {
  /// Creates the action.
  const SshConnectAction(this.hostId, this.terminal);

  /// Registered host id.
  final String hostId;

  /// Terminal to open the session in.
  final DevTerminal terminal;

  @override
  String get action => 'ssh_connect';

  @override
  Map<String, dynamic> fields() => {
        'host_id': hostId,
        'terminal': terminal.wire,
      };
}

/// A dev-action request with a client correlation id.
class DevActionRequest {
  /// Creates a request.
  const DevActionRequest({required this.requestId, required this.action});

  /// Correlation id echoed back in the result.
  final String requestId;

  /// The action to perform.
  final DevAction action;

  /// The flattened wire representation.
  Map<String, dynamic> toJson() => {
        'request_id': requestId,
        'action': action.action,
        ...action.fields(),
      };
}
