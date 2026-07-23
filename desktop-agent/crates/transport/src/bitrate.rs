//! Adaptive bitrate controller.
//!
//! A pure, deterministic congestion-response controller that adjusts the target
//! video bitrate from periodic network samples (packet loss, round-trip time,
//! and optionally the receiver's estimated available bandwidth). It follows a
//! loss-based AIMD (additive-increase / multiplicative-decrease) strategy,
//! similar in spirit to WebRTC's GCC loss controller, and recommends a
//! resolution/FPS tier for the chosen bitrate. Keeping it side-effect-free
//! makes it exhaustively unit-testable; the runtime feeds it samples and
//! applies the returned bitrate to the encoder / RTP sender.

/// Inclusive bounds for the target bitrate, in bits per second.
#[derive(Debug, Clone, Copy)]
pub struct BitrateLimits {
    /// Lowest bitrate we will drop to (keeps a usable low-res stream).
    pub min_bps: u32,
    /// Highest bitrate we will climb to.
    pub max_bps: u32,
    /// Bitrate to start at before the first sample.
    pub start_bps: u32,
}

impl Default for BitrateLimits {
    fn default() -> Self {
        // 300 kbps floor (audio + low-res video) up to 8 Mbps (1080p60-ish).
        Self {
            min_bps: 300_000,
            max_bps: 8_000_000,
            start_bps: 2_500_000,
        }
    }
}

/// A single network observation for one control interval.
#[derive(Debug, Clone, Copy)]
pub struct NetworkSample {
    /// Fraction of packets lost this interval, in `[0.0, 1.0]`.
    pub loss: f32,
    /// Smoothed round-trip time in milliseconds.
    pub rtt_ms: u32,
    /// Optional receiver-estimated available bandwidth (bps). When present it
    /// caps increases so we don't overshoot a known ceiling.
    pub estimated_bps: Option<u32>,
}

/// A recommended encoder resolution/FPS tier for a bitrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityTier {
    /// ~360p30 — congested / very low bitrate.
    Low,
    /// ~720p30 — moderate bitrate.
    Medium,
    /// ~1080p30 — good bitrate.
    High,
    /// ~1080p60+ — ample bitrate.
    Ultra,
}

/// The controller's decision for the next interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BitrateDecision {
    /// The new target bitrate in bps.
    pub target_bps: u32,
    /// The recommended quality tier for `target_bps`.
    pub tier: QualityTier,
}

/// Loss thresholds that trigger increase/hold/decrease (per WebRTC GCC's
/// well-known 2% / 10% loss breakpoints).
const LOSS_DECREASE: f32 = 0.10;
const LOSS_INCREASE: f32 = 0.02;
/// Multiplicative decrease factor applied on high loss.
const DECREASE_FACTOR: f32 = 0.85;
/// Additive increase fraction applied on low loss.
const INCREASE_FRACTION: f32 = 0.08;
/// RTT (ms) above which we suppress increases (bufferbloat guard).
const HIGH_RTT_MS: u32 = 400;

/// The adaptive bitrate controller.
#[derive(Debug, Clone)]
pub struct AdaptiveBitrateController {
    limits: BitrateLimits,
    target: u32,
}

impl AdaptiveBitrateController {
    /// Create a controller with the given limits, starting at `start_bps`
    /// (clamped into range).
    pub fn new(limits: BitrateLimits) -> Self {
        let target = limits.start_bps.clamp(limits.min_bps, limits.max_bps);
        Self { limits, target }
    }

    /// The current target bitrate (bps).
    pub fn target_bps(&self) -> u32 {
        self.target
    }

    /// Feed a network sample and return the updated decision.
    ///
    /// - loss ≥ 10%  → multiplicative decrease
    /// - loss ≤ 2% and RTT healthy → additive increase (capped by any estimate)
    /// - otherwise → hold
    pub fn observe(&mut self, sample: NetworkSample) -> BitrateDecision {
        let loss = sample.loss.clamp(0.0, 1.0);

        if loss >= LOSS_DECREASE {
            // Scale the decrease with the severity of loss for faster recovery.
            let severity = ((loss - LOSS_DECREASE) / (1.0 - LOSS_DECREASE)).clamp(0.0, 1.0);
            let factor = DECREASE_FACTOR - 0.25 * severity; // down to ~0.60 at 100% loss
            self.set_target((self.target as f32 * factor) as u32);
        } else if loss <= LOSS_INCREASE && sample.rtt_ms <= HIGH_RTT_MS {
            let mut next = self.target + (self.target as f32 * INCREASE_FRACTION) as u32;
            if let Some(est) = sample.estimated_bps {
                next = next.min(est.max(self.limits.min_bps));
            }
            self.set_target(next);
        }
        // else: moderate loss (2%–10%) → hold steady.

        BitrateDecision {
            target_bps: self.target,
            tier: tier_for(self.target),
        }
    }

    fn set_target(&mut self, value: u32) {
        self.target = value.clamp(self.limits.min_bps, self.limits.max_bps);
    }
}

/// Map a bitrate to a resolution/FPS tier.
fn tier_for(bps: u32) -> QualityTier {
    match bps {
        b if b < 800_000 => QualityTier::Low,
        b if b < 2_500_000 => QualityTier::Medium,
        b if b < 5_000_000 => QualityTier::High,
        _ => QualityTier::Ultra,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl() -> AdaptiveBitrateController {
        AdaptiveBitrateController::new(BitrateLimits::default())
    }

    #[test]
    fn high_loss_decreases_bitrate() {
        let mut c = ctrl();
        let start = c.target_bps();
        let d = c.observe(NetworkSample {
            loss: 0.20,
            rtt_ms: 50,
            estimated_bps: None,
        });
        assert!(d.target_bps < start, "expected decrease, {} !< {}", d.target_bps, start);
    }

    #[test]
    fn low_loss_increases_bitrate() {
        let mut c = ctrl();
        let start = c.target_bps();
        let d = c.observe(NetworkSample {
            loss: 0.0,
            rtt_ms: 30,
            estimated_bps: None,
        });
        assert!(d.target_bps > start);
    }

    #[test]
    fn moderate_loss_holds() {
        let mut c = ctrl();
        let start = c.target_bps();
        let d = c.observe(NetworkSample {
            loss: 0.05,
            rtt_ms: 30,
            estimated_bps: None,
        });
        assert_eq!(d.target_bps, start);
    }

    #[test]
    fn high_rtt_suppresses_increase() {
        let mut c = ctrl();
        let start = c.target_bps();
        let d = c.observe(NetworkSample {
            loss: 0.0,
            rtt_ms: 800,
            estimated_bps: None,
        });
        assert_eq!(d.target_bps, start, "bufferbloat guard should hold steady");
    }

    #[test]
    fn respects_min_and_max_bounds() {
        let limits = BitrateLimits {
            min_bps: 500_000,
            max_bps: 1_000_000,
            start_bps: 900_000,
        };
        let mut c = AdaptiveBitrateController::new(limits);
        // Drive way up.
        for _ in 0..50 {
            c.observe(NetworkSample {
                loss: 0.0,
                rtt_ms: 20,
                estimated_bps: None,
            });
        }
        assert!(c.target_bps() <= limits.max_bps);
        // Drive way down.
        for _ in 0..50 {
            c.observe(NetworkSample {
                loss: 0.9,
                rtt_ms: 20,
                estimated_bps: None,
            });
        }
        assert!(c.target_bps() >= limits.min_bps);
    }

    #[test]
    fn estimated_bandwidth_caps_increase() {
        let mut c = ctrl();
        let est = c.target_bps() + 10_000; // barely above current
        let d = c.observe(NetworkSample {
            loss: 0.0,
            rtt_ms: 20,
            estimated_bps: Some(est),
        });
        assert!(d.target_bps <= est);
    }

    #[test]
    fn start_bitrate_is_clamped_into_range() {
        let c = AdaptiveBitrateController::new(BitrateLimits {
            min_bps: 1_000_000,
            max_bps: 2_000_000,
            start_bps: 100, // below min
        });
        assert_eq!(c.target_bps(), 1_000_000);
    }

    #[test]
    fn tiers_map_expectedly() {
        assert_eq!(tier_for(500_000), QualityTier::Low);
        assert_eq!(tier_for(1_500_000), QualityTier::Medium);
        assert_eq!(tier_for(3_000_000), QualityTier::High);
        assert_eq!(tier_for(6_000_000), QualityTier::Ultra);
    }
}
