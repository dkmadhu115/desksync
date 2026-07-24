import 'package:flutter_riverpod/flutter_riverpod.dart';

import '../../../core/util/uuid.dart';
import '../domain/dev_action.dart';
import 'control_sink.dart';

/// Dispatches developer actions to the agent over the control channel.
///
/// Each action gets a fresh correlation id and is serialized to the wire
/// contract, then sent through the [SwitchableControlSink]. State is the running
/// count of actions sent, which the UI surfaces and tests assert on.
class DevActionController extends Notifier<int> {
  ControlSink get _sink => ref.read(switchableControlSinkProvider);

  @override
  int build() => 0;

  /// Send [action], returning the generated request id.
  String dispatch(DevAction action) {
    final requestId = generateUuidV4();
    final request = DevActionRequest(requestId: requestId, action: action);
    _sink.send(encodeControlFrame(request.toJson()));
    state += 1;
    return requestId;
  }

  /// Launch an editor, optionally in a workspace.
  String launchEditor(DevEditor editor, {String? workspaceId}) =>
      dispatch(LaunchEditorAction(editor, workspaceId: workspaceId));

  /// Open a terminal, optionally in a workspace.
  String openTerminal(DevTerminal terminal, {String? workspaceId}) =>
      dispatch(OpenTerminalAction(terminal, workspaceId: workspaceId));

  /// Run a tool shortcut, optionally in a workspace.
  String runShortcut(DevTool tool, String shortcutId, {String? workspaceId}) =>
      dispatch(RunShortcutAction(tool, shortcutId, workspaceId: workspaceId));

  /// Open an SSH session to a registered host.
  String sshConnect(String hostId, DevTerminal terminal) =>
      dispatch(SshConnectAction(hostId, terminal));
}

/// Provides the [DevActionController].
final devActionControllerProvider =
    NotifierProvider<DevActionController, int>(DevActionController.new);
