import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';

import '../../../app/router.dart';
import '../application/input_controller.dart';
import '../domain/input_event.dart';
import '../domain/key_codes.dart';
import '../domain/touch_mapping.dart';

/// Interaction mode for single-finger gestures on the remote surface.
enum ViewerGestureMode {
  /// Single-finger drag moves the remote pointer (direct touch).
  pointer,

  /// Single-finger drag scrolls the remote content.
  scroll,
}

/// The remote desktop viewer. The live WebRTC video track renders here in
/// Phase 5; in Phase 4 we render a placeholder surface and fully wire the touch
/// and keyboard controls, translating gestures into [InputEvent]s that flow
/// through the [InputController] to the input sink.
class DesktopViewerScreen extends ConsumerStatefulWidget {
  /// Creates the viewer for the given [deviceId].
  const DesktopViewerScreen({required this.deviceId, super.key});

  /// The device being controlled.
  final String deviceId;

  @override
  ConsumerState<DesktopViewerScreen> createState() =>
      _DesktopViewerScreenState();
}

class _DesktopViewerScreenState extends ConsumerState<DesktopViewerScreen> {
  ViewerGestureMode _mode = ViewerGestureMode.pointer;
  bool _rightClick = false;
  bool _keyboardVisible = false;

  final _keyboardController = TextEditingController();
  final _keyboardFocus = FocusNode();
  String _lastKeyboardText = '';

  @override
  void dispose() {
    _keyboardController.dispose();
    _keyboardFocus.dispose();
    super.dispose();
  }

  InputController get _input => ref.read(inputControllerProvider.notifier);

  @override
  Widget build(BuildContext context) {
    final eventCount = ref.watch(inputControllerProvider);

    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        title: Text('Device ${widget.deviceId}'),
        actions: [
          IconButton(
            tooltip: 'Disconnect',
            icon: const Icon(Icons.close),
            onPressed: () => context.go(Routes.devices),
          ),
        ],
      ),
      body: Column(
        children: [
          Expanded(
            child: _RemoteSurface(
              mode: _mode,
              rightClick: _rightClick,
              onClick: (p, button) => _input.click(p, button: button),
              onMove: _input.move,
              onScroll: _input.scroll,
            ),
          ),
          if (_keyboardVisible) _keyboardBar(context),
        ],
      ),
      bottomNavigationBar: _controlBar(eventCount),
    );
  }

  Widget _controlBar(int eventCount) {
    return BottomAppBar(
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceAround,
        children: [
          IconButton(
            tooltip: 'Keyboard',
            isSelected: _keyboardVisible,
            icon: const Icon(Icons.keyboard),
            onPressed: _toggleKeyboard,
          ),
          IconButton(
            tooltip: _mode == ViewerGestureMode.pointer
                ? 'Pointer mode (tap to switch to scroll)'
                : 'Scroll mode (tap to switch to pointer)',
            isSelected: _mode == ViewerGestureMode.scroll,
            icon: Icon(
              _mode == ViewerGestureMode.scroll
                  ? Icons.swap_vert
                  : Icons.mouse,
            ),
            onPressed: () => setState(() {
              _mode = _mode == ViewerGestureMode.pointer
                  ? ViewerGestureMode.scroll
                  : ViewerGestureMode.pointer;
            }),
          ),
          IconButton(
            tooltip: 'Right-click taps',
            isSelected: _rightClick,
            icon: const Icon(Icons.ads_click),
            onPressed: () => setState(() => _rightClick = !_rightClick),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 8),
            child: Text('$eventCount events'),
          ),
        ],
      ),
    );
  }

  Widget _keyboardBar(BuildContext context) {
    return Container(
      color: Theme.of(context).colorScheme.surface,
      padding: const EdgeInsets.fromLTRB(12, 8, 12, 8),
      child: Row(
        children: [
          Expanded(
            child: TextField(
              controller: _keyboardController,
              focusNode: _keyboardFocus,
              autofocus: true,
              autocorrect: false,
              enableSuggestions: false,
              decoration: const InputDecoration(
                isDense: true,
                border: OutlineInputBorder(),
                hintText: 'Type to send keys to the remote desktop',
              ),
              onChanged: _onKeyboardChanged,
              onSubmitted: (_) {
                _input.sendKey(HidKey.enter);
                _resetKeyboardField();
              },
            ),
          ),
          IconButton(
            tooltip: 'Backspace',
            icon: const Icon(Icons.backspace_outlined),
            onPressed: () => _input.sendKey(HidKey.backspace),
          ),
        ],
      ),
    );
  }

  void _toggleKeyboard() {
    setState(() => _keyboardVisible = !_keyboardVisible);
    if (_keyboardVisible) {
      _resetKeyboardField();
      _keyboardFocus.requestFocus();
    } else {
      _keyboardFocus.unfocus();
    }
  }

  /// Diff the text field against its previous value to derive typed characters
  /// and backspaces, forwarding each as remote key events. The field is a
  /// capture buffer, not a document, so it is periodically reset.
  void _onKeyboardChanged(String value) {
    if (value.length > _lastKeyboardText.length) {
      final added = value.substring(_lastKeyboardText.length);
      _input.typeText(added);
    } else if (value.length < _lastKeyboardText.length) {
      final removed = _lastKeyboardText.length - value.length;
      for (var i = 0; i < removed; i++) {
        _input.sendKey(HidKey.backspace);
      }
    }
    _lastKeyboardText = value;

    // Keep the buffer small so it doesn't grow unbounded during a session.
    if (value.length > 64) {
      _resetKeyboardField();
    }
  }

  void _resetKeyboardField() {
    _keyboardController.clear();
    _lastKeyboardText = '';
  }
}

/// The touch surface. Uses a [LayoutBuilder] so gesture positions can be
/// normalized against the actual rendered size.
class _RemoteSurface extends StatelessWidget {
  const _RemoteSurface({
    required this.mode,
    required this.rightClick,
    required this.onClick,
    required this.onMove,
    required this.onScroll,
  });

  final ViewerGestureMode mode;
  final bool rightClick;
  final void Function(NormalizedPoint p, PointerButton button) onClick;
  final void Function(NormalizedPoint p) onMove;
  final void Function(double dxPixels, double dyPixels) onScroll;

  @override
  Widget build(BuildContext context) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        final height = constraints.maxHeight;

        NormalizedPoint norm(Offset local) =>
            TouchMapping.normalize(local.dx, local.dy, width, height);

        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onTapUp: (d) => onClick(
            norm(d.localPosition),
            rightClick ? PointerButton.right : PointerButton.left,
          ),
          onLongPressStart: (d) =>
              onClick(norm(d.localPosition), PointerButton.right),
          onPanUpdate: (d) {
            if (mode == ViewerGestureMode.scroll) {
              onScroll(d.delta.dx, d.delta.dy);
            } else {
              onMove(norm(d.localPosition));
            }
          },
          child: Center(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const Icon(Icons.cast_connected,
                    color: Colors.white24, size: 64),
                const SizedBox(height: 12),
                Text(
                  'Remote stream renders here (Phase 5).\n'
                  '${mode == ViewerGestureMode.scroll ? "Scroll" : "Pointer"} mode • '
                  '${rightClick ? "right-click" : "left-click"} taps',
                  textAlign: TextAlign.center,
                  style: const TextStyle(color: Colors.white38),
                ),
              ],
            ),
          ),
        );
      },
    );
  }
}
