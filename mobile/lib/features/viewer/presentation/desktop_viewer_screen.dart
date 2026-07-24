import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_webrtc/flutter_webrtc.dart';
import 'package:go_router/go_router.dart';

import '../../../app/router.dart';
import '../application/input_controller.dart';
import '../application/viewer_controller.dart';
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

/// The remote desktop viewer. It resolves the device's active pairing, creates
/// a session, and drives a WebRTC connection: the live video track renders on
/// the surface while touch/keyboard gestures are translated into [InputEvent]s
/// that flow through the [InputController] to the live data-channel sink.
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

  ViewerController? _viewer;

  @override
  void initState() {
    super.initState();
    // Build and start the connection once the first frame is scheduled, so the
    // providers are available and the widget is mounted.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      final controller =
          ref.read(viewerControllerFactoryProvider)(widget.deviceId);
      setState(() => _viewer = controller);
      controller.connect();
    });
  }

  @override
  void dispose() {
    _viewer?.dispose();
    _keyboardController.dispose();
    _keyboardFocus.dispose();
    super.dispose();
  }

  InputController get _input => ref.read(inputControllerProvider.notifier);

  @override
  Widget build(BuildContext context) {
    final eventCount = ref.watch(inputControllerProvider);
    final viewer = _viewer;

    return Scaffold(
      backgroundColor: Colors.black,
      appBar: AppBar(
        title: Text('Device ${widget.deviceId}'),
        actions: [
          IconButton(
            tooltip: 'Quick Launch',
            icon: const Icon(Icons.rocket_launch),
            onPressed: () => context.push(Routes.quickLaunch),
          ),
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
              viewer: viewer,
              onRetry: _reconnect,
              onPair: () => context.go(Routes.pairing),
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

  void _reconnect() {
    final controller =
        ref.read(viewerControllerFactoryProvider)(widget.deviceId);
    final previous = _viewer;
    setState(() => _viewer = controller);
    previous?.dispose();
    controller.connect();
  }

  Future<void> _sendClipboard() async {
    final data = await Clipboard.getData(Clipboard.kTextPlain);
    final text = data?.text;
    if (text == null || text.isEmpty) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Clipboard is empty.')),
      );
      return;
    }
    _input.setClipboard(text);
    if (!mounted) return;
    ScaffoldMessenger.of(context).showSnackBar(
      const SnackBar(content: Text('Clipboard sent to the desktop.')),
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
          IconButton(
            tooltip: 'Send clipboard to desktop',
            icon: const Icon(Icons.content_paste_go),
            onPressed: _sendClipboard,
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

/// The remote surface. Renders the live WebRTC video once connected and a
/// status overlay otherwise, and forwards gestures as normalized input while
/// connected. Uses a [LayoutBuilder] so gesture positions are normalized
/// against the actual rendered size.
class _RemoteSurface extends StatelessWidget {
  const _RemoteSurface({
    required this.mode,
    required this.rightClick,
    required this.viewer,
    required this.onRetry,
    required this.onPair,
    required this.onClick,
    required this.onMove,
    required this.onScroll,
  });

  final ViewerGestureMode mode;
  final bool rightClick;
  final ViewerController? viewer;
  final VoidCallback onRetry;
  final VoidCallback onPair;
  final void Function(NormalizedPoint p, PointerButton button) onClick;
  final void Function(NormalizedPoint p) onMove;
  final void Function(double dxPixels, double dyPixels) onScroll;

  @override
  Widget build(BuildContext context) {
    final controller = viewer;
    if (controller == null) {
      return const _StatusOverlay(
        icon: Icons.hourglass_top,
        message: 'Preparing…',
        busy: true,
      );
    }

    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        switch (controller.phase) {
          case ViewerPhase.resolving:
            return const _StatusOverlay(
              icon: Icons.search,
              message: 'Finding your paired desktop…',
              busy: true,
            );
          case ViewerPhase.connecting:
            return const _StatusOverlay(
              icon: Icons.cast_connected,
              message: 'Connecting to the desktop…',
              busy: true,
            );
          case ViewerPhase.noPairing:
            return _StatusOverlay(
              icon: Icons.link_off,
              message: 'This desktop isn’t paired with your phone yet.',
              actionLabel: 'Pair a device',
              onAction: onPair,
            );
          case ViewerPhase.failed:
            return _StatusOverlay(
              icon: Icons.error_outline,
              message: controller.errorMessage ?? 'Connection failed.',
              actionLabel: 'Retry',
              onAction: onRetry,
            );
          case ViewerPhase.closed:
            return _StatusOverlay(
              icon: Icons.cancel_outlined,
              message: 'The session ended.',
              actionLabel: 'Reconnect',
              onAction: onRetry,
            );
          case ViewerPhase.connected:
            return _liveSurface(controller);
        }
      },
    );
  }

  Widget _liveSurface(ViewerController controller) {
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        final height = constraints.maxHeight;

        NormalizedPoint norm(Offset local) =>
            TouchMapping.normalize(local.dx, local.dy, width, height);

        final renderer = controller.renderer;

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
          child: renderer == null
              ? const ColoredBox(color: Colors.black)
              : RTCVideoView(
                  renderer,
                  objectFit:
                      RTCVideoViewObjectFit.RTCVideoViewObjectFitContain,
                ),
        );
      },
    );
  }
}

/// A centered status overlay with an icon, message, and optional action.
class _StatusOverlay extends StatelessWidget {
  const _StatusOverlay({
    required this.icon,
    required this.message,
    this.busy = false,
    this.actionLabel,
    this.onAction,
  });

  final IconData icon;
  final String message;
  final bool busy;
  final String? actionLabel;
  final VoidCallback? onAction;

  @override
  Widget build(BuildContext context) {
    return Center(
      child: Padding(
        padding: const EdgeInsets.all(24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Icon(icon, color: Colors.white24, size: 64),
            const SizedBox(height: 16),
            Text(
              message,
              textAlign: TextAlign.center,
              style: const TextStyle(color: Colors.white70),
            ),
            if (busy) ...[
              const SizedBox(height: 20),
              const SizedBox(
                width: 24,
                height: 24,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
            ],
            if (actionLabel != null && onAction != null) ...[
              const SizedBox(height: 20),
              FilledButton(onPressed: onAction, child: Text(actionLabel!)),
            ],
          ],
        ),
      ),
    );
  }
}
