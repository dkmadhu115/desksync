/// A pure, deterministic adaptive-bitrate controller, mirroring the Rust
/// agent's `AdaptiveBitrateController`. It maps periodic network samples
/// (packet loss, RTT, optional bandwidth estimate) to a target video bitrate
/// using loss-based AIMD (additive increase / multiplicative decrease). On the
/// mobile side it drives the receiver's max-bitrate hint / decode expectations;
/// keeping it side-effect-free makes it exhaustively unit-testable.
library;

/// Inclusive bitrate bounds (bits per second).
class BitrateLimits {
  /// Creates limits.
  const BitrateLimits({
    this.minBps = 300000,
    this.maxBps = 8000000,
    this.startBps = 2500000,
  });

  /// Lowest bitrate.
  final int minBps;

  /// Highest bitrate.
  final int maxBps;

  /// Starting bitrate.
  final int startBps;
}

/// A recommended resolution/FPS tier.
enum QualityTier {
  /// ~360p30.
  low,

  /// ~720p30.
  medium,

  /// ~1080p30.
  high,

  /// ~1080p60+.
  ultra,
}

/// One network observation for a control interval.
class NetworkSample {
  /// Creates a sample.
  const NetworkSample({required this.loss, required this.rttMs, this.estimatedBps});

  /// Fraction of packets lost, `[0,1]`.
  final double loss;

  /// Smoothed RTT in ms.
  final int rttMs;

  /// Optional receiver-estimated available bandwidth (bps).
  final int? estimatedBps;
}

/// The controller's decision.
class BitrateDecision {
  /// Creates a decision.
  const BitrateDecision({required this.targetBps, required this.tier});

  /// New target bitrate (bps).
  final int targetBps;

  /// Recommended tier for [targetBps].
  final QualityTier tier;
}

/// Loss-based AIMD adaptive bitrate controller.
class AdaptiveBitrateController {
  /// Creates a controller starting at [BitrateLimits.startBps] (clamped).
  AdaptiveBitrateController([this.limits = const BitrateLimits()])
      : _target = _clamp(limits.startBps, limits.minBps, limits.maxBps);

  /// The configured bounds.
  final BitrateLimits limits;
  int _target;

  static const double _lossDecrease = 0.10;
  static const double _lossIncrease = 0.02;
  static const double _decreaseFactor = 0.85;
  static const double _increaseFraction = 0.08;
  static const int _highRttMs = 400;

  /// Current target bitrate (bps).
  int get targetBps => _target;

  /// Feed a sample and return the updated decision.
  BitrateDecision observe(NetworkSample sample) {
    final loss = sample.loss.clamp(0.0, 1.0);

    if (loss >= _lossDecrease) {
      final severity = ((loss - _lossDecrease) / (1.0 - _lossDecrease)).clamp(0.0, 1.0);
      final factor = _decreaseFactor - 0.25 * severity;
      _setTarget((_target * factor).round());
    } else if (loss <= _lossIncrease && sample.rttMs <= _highRttMs) {
      var next = _target + (_target * _increaseFraction).round();
      final est = sample.estimatedBps;
      if (est != null) {
        final ceiling = est < limits.minBps ? limits.minBps : est;
        next = next < ceiling ? next : ceiling;
      }
      _setTarget(next);
    }
    // Moderate loss (2%–10%): hold steady.

    return BitrateDecision(targetBps: _target, tier: tierFor(_target));
  }

  void _setTarget(int value) {
    _target = _clamp(value, limits.minBps, limits.maxBps);
  }

  static int _clamp(int v, int lo, int hi) => v < lo ? lo : (v > hi ? hi : v);
}

/// Map a bitrate to a resolution/FPS tier.
QualityTier tierFor(int bps) {
  if (bps < 800000) return QualityTier.low;
  if (bps < 2500000) return QualityTier.medium;
  if (bps < 5000000) return QualityTier.high;
  return QualityTier.ultra;
}
