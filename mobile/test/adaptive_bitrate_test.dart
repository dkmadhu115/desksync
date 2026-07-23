import 'package:desksync_mobile/features/viewer/application/adaptive_bitrate.dart';
import 'package:flutter_test/flutter_test.dart';

void main() {
  AdaptiveBitrateController ctrl() => AdaptiveBitrateController();

  test('high loss decreases bitrate', () {
    final c = ctrl();
    final start = c.targetBps;
    final d = c.observe(const NetworkSample(loss: 0.2, rttMs: 50));
    expect(d.targetBps, lessThan(start));
  });

  test('low loss increases bitrate', () {
    final c = ctrl();
    final start = c.targetBps;
    final d = c.observe(const NetworkSample(loss: 0.0, rttMs: 30));
    expect(d.targetBps, greaterThan(start));
  });

  test('moderate loss holds steady', () {
    final c = ctrl();
    final start = c.targetBps;
    final d = c.observe(const NetworkSample(loss: 0.05, rttMs: 30));
    expect(d.targetBps, start);
  });

  test('high RTT suppresses increase (bufferbloat guard)', () {
    final c = ctrl();
    final start = c.targetBps;
    final d = c.observe(const NetworkSample(loss: 0.0, rttMs: 800));
    expect(d.targetBps, start);
  });

  test('respects min and max bounds', () {
    final c = AdaptiveBitrateController(
      const BitrateLimits(minBps: 500000, maxBps: 1000000, startBps: 900000),
    );
    for (var i = 0; i < 50; i++) {
      c.observe(const NetworkSample(loss: 0.0, rttMs: 20));
    }
    expect(c.targetBps, lessThanOrEqualTo(1000000));
    for (var i = 0; i < 50; i++) {
      c.observe(const NetworkSample(loss: 0.9, rttMs: 20));
    }
    expect(c.targetBps, greaterThanOrEqualTo(500000));
  });

  test('estimated bandwidth caps the increase', () {
    final c = ctrl();
    final est = c.targetBps + 10000;
    final d = c.observe(NetworkSample(loss: 0.0, rttMs: 20, estimatedBps: est));
    expect(d.targetBps, lessThanOrEqualTo(est));
  });

  test('start bitrate is clamped into range', () {
    final c = AdaptiveBitrateController(
      const BitrateLimits(minBps: 1000000, maxBps: 2000000, startBps: 100),
    );
    expect(c.targetBps, 1000000);
  });

  test('tiers map as expected', () {
    expect(tierFor(500000), QualityTier.low);
    expect(tierFor(1500000), QualityTier.medium);
    expect(tierFor(3000000), QualityTier.high);
    expect(tierFor(6000000), QualityTier.ultra);
  });
}
