//! Screen capture abstraction.
//!
//! The [`ScreenCapturer`] trait hides the per-platform capture backend:
//! ScreenCaptureKit on macOS, DXGI Desktop Duplication on Windows, and PipeWire
//! on Linux. Phase 1 ships the trait, a [`Monitor`]/[`Frame`] model, and a
//! [`NoopCapturer`] so the agent runtime compiles and is testable. Real
//! backends are implemented in Phase 3.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use async_trait::async_trait;
use desksync_core::error::Result;
use desksync_core::subsystem::{HealthStatus, Subsystem};
use std::sync::atomic::{AtomicBool, Ordering};

/// A display attached to the host, enumerated for multi-monitor support.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Monitor {
    /// Platform-specific monitor identifier.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// Width in physical pixels.
    pub width: u32,
    /// Height in physical pixels.
    pub height: u32,
    /// Whether this is the primary display.
    pub primary: bool,
}

/// A single captured frame in a raw, pre-encode pixel format (BGRA).
#[derive(Debug, Clone)]
pub struct Frame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Monotonic capture timestamp in microseconds.
    pub timestamp_us: u64,
    /// Raw BGRA pixel buffer (len == width*height*4).
    pub data: Vec<u8>,
}

/// Backend-agnostic screen capture interface.
#[async_trait]
pub trait ScreenCapturer: Subsystem {
    /// Enumerate the available monitors.
    async fn monitors(&self) -> Result<Vec<Monitor>>;

    /// Capture the next frame from the given monitor id.
    async fn capture(&self, monitor_id: &str) -> Result<Frame>;
}

/// A no-op capturer used in tests and on platforms without a backend wired yet.
/// It reports a single synthetic monitor and returns blank frames.
#[derive(Debug, Default)]
pub struct NoopCapturer {
    running: AtomicBool,
}

impl NoopCapturer {
    /// Create a new no-op capturer.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Subsystem for NoopCapturer {
    fn name(&self) -> &'static str {
        "capture"
    }
    async fn start(&self) -> Result<()> {
        self.running.store(true, Ordering::SeqCst);
        tracing::debug!("noop capturer started");
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }
    async fn health(&self) -> HealthStatus {
        if self.running.load(Ordering::SeqCst) {
            HealthStatus::Healthy
        } else {
            HealthStatus::Stopped
        }
    }
}

#[async_trait]
impl ScreenCapturer for NoopCapturer {
    async fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(vec![Monitor {
            id: "0".into(),
            name: "synthetic".into(),
            width: 1920,
            height: 1080,
            primary: true,
        }])
    }
    async fn capture(&self, _monitor_id: &str) -> Result<Frame> {
        Ok(Frame {
            width: 1920,
            height: 1080,
            timestamp_us: 0,
            data: vec![0u8; 1920 * 1080 * 4],
        })
    }
}

/// Platform backend selection. Concrete implementations land in Phase 3.
pub mod platform {
    /// Name of the capture backend selected for the current target OS.
    #[cfg(target_os = "macos")]
    pub const BACKEND: &str = "ScreenCaptureKit";
    /// Name of the capture backend selected for the current target OS.
    #[cfg(target_os = "windows")]
    pub const BACKEND: &str = "DXGI Desktop Duplication";
    /// Name of the capture backend selected for the current target OS.
    #[cfg(target_os = "linux")]
    pub const BACKEND: &str = "PipeWire";
    /// Name of the capture backend selected for the current target OS.
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    pub const BACKEND: &str = "unsupported";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_capturer_lifecycle_and_capture() {
        let c = NoopCapturer::new();
        assert_eq!(c.health().await, HealthStatus::Stopped);
        c.start().await.unwrap();
        assert_eq!(c.health().await, HealthStatus::Healthy);

        let monitors = c.monitors().await.unwrap();
        assert_eq!(monitors.len(), 1);
        assert!(monitors[0].primary);

        let frame = c.capture("0").await.unwrap();
        assert_eq!(frame.data.len(), (frame.width * frame.height * 4) as usize);

        c.stop().await.unwrap();
        assert_eq!(c.health().await, HealthStatus::Stopped);
    }

    #[test]
    fn backend_is_named() {
        assert!(!platform::BACKEND.is_empty());
    }
}
