import 'input_event.dart';

/// A coordinate normalized to the remote display, each axis in `[0,1]`.
class NormalizedPoint {
  /// Creates a normalized point.
  const NormalizedPoint(this.x, this.y);

  /// Normalized X in [0,1].
  final double x;

  /// Normalized Y in [0,1].
  final double y;
}

/// Pure geometry + gesture→event translation for the remote input surface.
///
/// All methods are side-effect free and take raw dimensions (no Flutter types)
/// so they can be unit-tested exhaustively. The viewer widget feeds real
/// gesture data in; the produced [InputEvent]s are handed to the input sink.
abstract final class TouchMapping {
  /// Convert a local touch position within a `surfaceWidth`×`surfaceHeight`
  /// widget into a normalized remote coordinate, clamped to `[0,1]`.
  static NormalizedPoint normalize(
    double localX,
    double localY,
    double surfaceWidth,
    double surfaceHeight,
  ) {
    if (surfaceWidth <= 0 || surfaceHeight <= 0) {
      return const NormalizedPoint(0, 0);
    }
    return NormalizedPoint(
      _clamp01(localX / surfaceWidth),
      _clamp01(localY / surfaceHeight),
    );
  }

  /// Events for a tap/click at [p]: move the pointer there, then press and
  /// release the given [button].
  static List<InputEvent> click(
    NormalizedPoint p, {
    PointerButton button = PointerButton.left,
    Modifiers modifiers = Modifiers.none,
  }) {
    return [
      MouseMoveEvent(x: p.x, y: p.y),
      MouseButtonEvent(button: button, pressed: true, modifiers: modifiers),
      MouseButtonEvent(button: button, pressed: false, modifiers: modifiers),
    ];
  }

  /// A single pointer move to [p].
  static InputEvent move(NormalizedPoint p) => MouseMoveEvent(x: p.x, y: p.y);

  /// Button press (used at the start of a drag).
  static InputEvent buttonDown(
    NormalizedPoint p, {
    PointerButton button = PointerButton.left,
  }) =>
      MouseButtonEvent(button: button, pressed: true);

  /// Button release (used at the end of a drag).
  static InputEvent buttonUp({PointerButton button = PointerButton.left}) =>
      MouseButtonEvent(button: button, pressed: false);

  /// A scroll event from pixel deltas, scaled into wheel units. Vertical scroll
  /// is negated so that dragging content up scrolls down, matching native
  /// touch-scroll expectations. [scale] converts pixels→wheel notches.
  static ScrollEvent scroll(
    double dxPixels,
    double dyPixels, {
    double scale = 0.1,
  }) {
    return ScrollEvent(dx: dxPixels * scale, dy: -dyPixels * scale);
  }

  static double _clamp01(double v) => v < 0 ? 0 : (v > 1 ? 1 : v);
}
