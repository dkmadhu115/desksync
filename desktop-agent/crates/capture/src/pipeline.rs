//! The capture loop: a lifecycle-managed [`Subsystem`] that captures frames
//! from a [`ScreenCapturer`] at a target frame rate, downscales them to the
//! configured height, and publishes the latest frame to subscribers.
//!
//! It is generic over the capturer trait object, so it runs identically with
//! the real backend or the [`crate::NoopCapturer`]; the loop's timing and
//! back-pressure behaviour are therefore unit-tested without any display.

use crate::frame::downscale_to_max_height;
use crate::{Frame, ScreenCapturer};
use async_trait::async_trait;
use desksync_core::error::Result;
use desksync_core::subsystem::{HealthStatus, Subsystem};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::watch;
use tokio::task::JoinHandle;

/// Runtime settings for the capture loop.
#[derive(Debug, Clone)]
pub struct CaptureSettings {
    /// Monitor to capture; `None` selects the primary display at start.
    pub monitor_id: Option<String>,
    /// Target frames per second (clamped to at least 1).
    pub target_fps: u32,
    /// Maximum output height in pixels; larger frames are downscaled.
    pub max_height: u32,
}

impl Default for CaptureSettings {
    fn default() -> Self {
        Self {
            monitor_id: None,
            target_fps: 30,
            max_height: 1080,
        }
    }
}

/// A running (or idle) capture loop.
pub struct CaptureLoop {
    capturer: Arc<dyn ScreenCapturer>,
    settings: CaptureSettings,
    latest_tx: watch::Sender<Option<Arc<Frame>>>,
    latest_rx: watch::Receiver<Option<Arc<Frame>>>,
    frames: Arc<AtomicU64>,
    running: Arc<AtomicBool>,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl CaptureLoop {
    /// Build a capture loop over the given capturer and settings.
    pub fn new(capturer: Arc<dyn ScreenCapturer>, settings: CaptureSettings) -> Self {
        let (latest_tx, latest_rx) = watch::channel(None);
        Self {
            capturer,
            settings,
            latest_tx,
            latest_rx,
            frames: Arc::new(AtomicU64::new(0)),
            running: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }

    /// Subscribe to the latest captured frame. The receiver always observes the
    /// most recent frame (lossy/coalescing), which is the correct semantics for
    /// live video: a slow consumer skips stale frames rather than lagging.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<Frame>>> {
        self.latest_rx.clone()
    }

    /// Total number of frames captured since start.
    pub fn frames_captured(&self) -> u64 {
        self.frames.load(Ordering::SeqCst)
    }

    fn period(&self) -> Duration {
        let fps = self.settings.target_fps.max(1);
        Duration::from_secs_f64(1.0 / f64::from(fps))
    }

    /// Resolve the monitor to capture: the configured one, else the primary,
    /// else the first enumerated monitor.
    async fn resolve_monitor(&self) -> Result<String> {
        if let Some(id) = &self.settings.monitor_id {
            return Ok(id.clone());
        }
        let monitors = self.capturer.monitors().await?;
        let chosen = monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| monitors.first())
            .map(|m| m.id.clone())
            .ok_or_else(|| desksync_core::error::AgentError::subsystem("capture", "no monitors available"))?;
        Ok(chosen)
    }
}

#[async_trait]
impl Subsystem for CaptureLoop {
    fn name(&self) -> &'static str {
        "capture-pipeline"
    }

    async fn start(&self) -> Result<()> {
        // Idempotent: a second start while running is a no-op.
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let monitor_id = match self.resolve_monitor().await {
            Ok(id) => id,
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                return Err(e);
            }
        };

        let capturer = Arc::clone(&self.capturer);
        let tx = self.latest_tx.clone();
        let frames = Arc::clone(&self.frames);
        let running = Arc::clone(&self.running);
        let period = self.period();
        let max_height = self.settings.max_height;

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tracing::info!(monitor = %monitor_id, ?period, "capture loop started");

            while running.load(Ordering::SeqCst) {
                ticker.tick().await;
                match capturer.capture(&monitor_id).await {
                    Ok(frame) => {
                        let out = downscale_to_max_height(&frame, max_height);
                        frames.fetch_add(1, Ordering::SeqCst);
                        // Coalescing send: ignore "no receivers" errors.
                        let _ = tx.send(Some(Arc::new(out)));
                    }
                    Err(e) => {
                        // A transient capture error (e.g. display sleep) should
                        // not kill the loop; log and retry on the next tick.
                        tracing::warn!(error = %e, "frame capture failed; retrying");
                    }
                }
            }
            tracing::info!("capture loop stopped");
        });

        *self.task.lock().expect("capture task mutex poisoned") = Some(handle);
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        let handle = self.task.lock().expect("capture task mutex poisoned").take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NoopCapturer;

    #[tokio::test]
    async fn captures_and_publishes_downscaled_frames() {
        let capturer = Arc::new(NoopCapturer::new());
        let cap_loop = CaptureLoop::new(
            capturer,
            CaptureSettings {
                monitor_id: None,
                target_fps: 120,
                max_height: 100,
            },
        );

        let mut rx = cap_loop.subscribe();
        assert_eq!(cap_loop.health().await, HealthStatus::Stopped);
        cap_loop.start().await.unwrap();
        assert_eq!(cap_loop.health().await, HealthStatus::Healthy);

        // Wait for the first frame to be published.
        tokio::time::timeout(Duration::from_secs(2), rx.changed())
            .await
            .expect("frame should arrive within timeout")
            .expect("sender should be alive");

        let frame = rx.borrow().clone().expect("a frame is present");
        // 1920x1080 synthetic frame downscaled to a 100px height.
        assert_eq!(frame.height, 100);
        assert!(frame.is_valid());

        cap_loop.stop().await.unwrap();
        assert_eq!(cap_loop.health().await, HealthStatus::Stopped);
        assert!(cap_loop.frames_captured() >= 1);
    }

    #[tokio::test]
    async fn start_is_idempotent() {
        let capturer = Arc::new(NoopCapturer::new());
        let cap_loop = CaptureLoop::new(capturer, CaptureSettings::default());
        cap_loop.start().await.unwrap();
        cap_loop.start().await.unwrap(); // no panic, no second task
        cap_loop.stop().await.unwrap();
    }
}
