import 'dart:convert';

import 'package:desksync_mobile/features/devtools/application/control_sink.dart';
import 'package:desksync_mobile/features/devtools/application/dev_action_controller.dart';
import 'package:desksync_mobile/features/devtools/domain/dev_action.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

class _RecordingSink implements ControlSink {
  final frames = <String>[];

  @override
  void send(String jsonFrame) => frames.add(jsonFrame);
}

void main() {
  test('dispatch serializes the action and increments the count', () {
    final target = _RecordingSink();
    final switchable = SwitchableControlSink()..attach(target);
    final container = ProviderContainer(
      overrides: [
        switchableControlSinkProvider.overrideWithValue(switchable),
      ],
    );
    addTearDown(container.dispose);

    final controller = container.read(devActionControllerProvider.notifier);
    final id = controller.runShortcut(DevTool.docker, 'ps');

    expect(container.read(devActionControllerProvider), 1);
    expect(target.frames, hasLength(1));

    final json = jsonDecode(target.frames.single) as Map<String, dynamic>;
    expect(json['request_id'], id);
    expect(json['action'], 'run_shortcut');
    expect(json['tool'], 'docker');
    expect(json['shortcut_id'], 'ps');
  });

  test('every dispatch generates a unique request id', () {
    final target = _RecordingSink();
    final switchable = SwitchableControlSink()..attach(target);
    final container = ProviderContainer(
      overrides: [
        switchableControlSinkProvider.overrideWithValue(switchable),
      ],
    );
    addTearDown(container.dispose);

    final controller = container.read(devActionControllerProvider.notifier);
    final id1 = controller.launchEditor(DevEditor.cursor);
    final id2 = controller.launchEditor(DevEditor.cursor);

    expect(id1, isNot(id2));
    expect(container.read(devActionControllerProvider), 2);
  });
}
