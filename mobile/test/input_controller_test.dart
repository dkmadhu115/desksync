import 'package:desksync_mobile/features/viewer/application/input_controller.dart';
import 'package:desksync_mobile/features/viewer/application/input_sink.dart';
import 'package:desksync_mobile/features/viewer/domain/input_event.dart';
import 'package:desksync_mobile/features/viewer/domain/touch_mapping.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';

import 'support/fakes.dart';

void main() {
  late RecordingInputSink sink;
  late ProviderContainer container;

  setUp(() {
    sink = RecordingInputSink();
    container = ProviderContainer(
      overrides: [inputSinkProvider.overrideWithValue(sink)],
    );
  });
  tearDown(() => container.dispose());

  InputController controller() =>
      container.read(inputControllerProvider.notifier);

  test('click dispatches move+press+release and updates the counter', () {
    controller().click(const NormalizedPoint(0.5, 0.5));
    expect(sink.events, hasLength(3));
    expect(container.read(inputControllerProvider), 3);
  });

  test('drag lifecycle emits move, buttonDown, moves, buttonUp', () {
    final c = controller();
    c.dragStart(const NormalizedPoint(0.1, 0.1));
    c.dragUpdate(const NormalizedPoint(0.2, 0.2));
    c.dragEnd();

    expect(sink.events.whereType<MouseMoveEvent>(), hasLength(2));
    final buttons = sink.events.whereType<MouseButtonEvent>().toList();
    expect(buttons.first.pressed, isTrue);
    expect(buttons.last.pressed, isFalse);
  });

  test('typeText emits press/release pairs per character', () {
    controller().typeText('Ab');
    // 'A' -> press+release, 'b' -> press+release = 4 events.
    expect(sink.events, hasLength(4));
    expect(sink.events.whereType<KeyEvent>(), hasLength(4));
    final first = sink.events.first as KeyEvent;
    expect(first.modifiers.shift, isTrue); // 'A' needs shift
  });

  test('unmapped characters are skipped', () {
    controller().typeText('€');
    expect(sink.events, isEmpty);
  });

  test('setClipboard emits a clipboard_text event', () {
    controller().setClipboard('hello');
    expect(sink.events.single, isA<ClipboardTextEvent>());
  });
}
