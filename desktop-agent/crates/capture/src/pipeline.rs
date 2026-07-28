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
use std::time::{Duration, Instant};
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
        resolve_monitor(&self.capturer, self.settings.monitor_id.as_deref()).await
    }
}

/// Pick a monitor to capture.
///
/// A free function rather than a method because the capture loop needs it too: it
/// re-resolves after a failure, and it only holds the capturer.
async fn resolve_monitor(capturer: &Arc<dyn ScreenCapturer>, configured: Option<&str>) -> Result<String> {
    if let Some(id) = configured {
        return Ok(id.to_string());
    }
    let monitors = capturer.monitors().await?;
    monitors
        .iter()
        .find(|m| m.primary)
        .or_else(|| monitors.first())
        .map(|m| m.id.clone())
        .ok_or_else(|| desksync_core::error::AgentError::subsystem("capture", "no monitors available"))
}

/// How long to wait between complaints about a continuing capture failure.
///
/// The loop ticks up to 30 times a second, so logging every failure turns a display
/// being asleep into thousands of identical lines that bury everything else.
const FAILURE_LOG_INTERVAL: Duration = Duration::from_secs(30);

/// How often to re-check which monitor exists while capture is failing. Frequent
/// enough to recover promptly, rare enough not to hammer the window server.
const RERESOLVE_INTERVAL: Duration = Duration::from_secs(1);

/// Collapses a run of identical capture failures into occasional log lines.
struct FailureRun {
    consecutive: u64,
    last_logged: Option<Instant>,
}

impl FailureRun {
    fn new() -> Self {
        Self {
            consecutive: 0,
            last_logged: None,
        }
    }

    /// Record a failure, returning whether it is worth logging.
    fn record(&mut self) -> bool {
        self.consecutive += 1;
        let due = match self.last_logged {
            None => true,
            Some(at) => at.elapsed() >= FAILURE_LOG_INTERVAL,
        };
        if due {
            self.last_logged = Some(Instant::now());
        }
        due
    }

    /// Note a success, returning how many failures it ended (0 if none).
    fn clear(&mut self) -> u64 {
        let had = self.consecutive;
        self.consecutive = 0;
        self.last_logged = None;
        had
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
        let configured = self.settings.monitor_id.clone();

        let handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(period);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            tracing::info!(monitor = %monitor_id, ?period, "capture loop started");

            let mut monitor_id = monitor_id;
            let mut failures = FailureRun::new();
            let mut last_reresolve: Option<Instant> = None;

            while running.load(Ordering::SeqCst) {
                ticker.tick().await;
                match capturer.capture(&monitor_id).await {
                    Ok(frame) => {
                        let recovered = failures.clear();
                        if recovered > 0 {
                            tracing::info!(
                                monitor = %monitor_id,
                                failed_attempts = recovered,
                                "capture recovered"
                            );
                        }
                        let out = downscale_to_max_height(&frame, max_height);
                        frames.fetch_add(1, Ordering::SeqCst);
                        // Coalescing send: ignore "no receivers" errors.
                        let _ = tx.send(Some(Arc::new(out)));
                    }
                    Err(e) => {
                        // A capture error is usually the display going away — asleep,
                        // unplugged, or resolution changed — so the loop must survive
                        // it rather than exit.
                        if failures.record() {
                            tracing::warn!(
                                error = %e,
                                monitor = %monitor_id,
                                consecutive = failures.consecutive,
                                "frame capture failing; retrying"
                            );
                        }

                        // Crucially, also stop trusting the monitor id. macOS can hand
                        // out a different display id after sleep or a display change,
                        // and the old one then never becomes valid again — capture
                        // would fail forever while the screen is perfectly available.
                        // Skipped when the user pinned a specific monitor: honouring
                        // that choice matters more than capturing something else.
                        let due = last_reresolve.is_none_or(|at| at.elapsed() >= RERESOLVE_INTERVAL);
                        if configured.is_none() && due {
                            last_reresolve = Some(Instant::now());
                            if let Ok(found) = resolve_monitor(&capturer, None).await {
                                if found != monitor_id {
                                    tracing::info!(
                                        previous = %monitor_id,
                                        monitor = %found,
                                        "display changed; following it"
                                    );
                                    monitor_id = found;
                                }
                            }
                        }
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

    /// A capturer whose display id changes, the way macOS renumbers displays after
    /// sleep or a monitor change. It only serves frames for its current id.
    struct RenumberingCapturer {
        current: Mutex<String>,
        rejected: AtomicU64,
    }

    impl RenumberingCapturer {
        fn new(id: &str) -> Self {
            Self {
                current: Mutex::new(id.to_string()),
                rejected: AtomicU64::new(0),
            }
        }

        fn renumber(&self, id: &str) {
            *self.current.lock().unwrap() = id.to_string();
        }
    }

    #[async_trait]
    impl Subsystem for RenumberingCapturer {
        fn name(&self) -> &'static str {
            "capture"
        }
        async fn start(&self) -> Result<()> {
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            Ok(())
        }
        async fn health(&self) -> HealthStatus {
            HealthStatus::Healthy
        }
    }

    #[async_trait]
    impl ScreenCapturer for RenumberingCapturer {
        async fn monitors(&self) -> Result<Vec<crate::Monitor>> {
            Ok(vec![crate::Monitor {
                id: self.current.lock().unwrap().clone(),
                name: "display".into(),
                width: 320,
                height: 200,
                primary: true,
            }])
        }

        async fn capture(&self, monitor_id: &str) -> Result<Frame> {
            if monitor_id != self.current.lock().unwrap().as_str() {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                return Err(desksync_core::error::AgentError::subsystem(
                    "capture",
                    format!("monitor '{monitor_id}' not found"),
                ));
            }
            Ok(Frame {
                width: 320,
                height: 200,
                data: vec![0u8; 320 * 200 * 4],
                timestamp_us: 0,
            })
        }
    }

    #[tokio::test]
    async fn capture_follows_a_display_that_gets_renumbered() {
        // Observed in the wild: the display slept, its id stopped resolving, and the
        // loop failed 320 times in a row because the id was pinned at startup. The
        // screen was fine — only the number had changed.
        let capturer = Arc::new(RenumberingCapturer::new("display-1"));
        let cap_loop = CaptureLoop::new(
            Arc::clone(&capturer) as Arc<dyn ScreenCapturer>,
            CaptureSettings {
                monitor_id: None,
                target_fps: 120,
                max_height: 200,
            },
        );

        cap_loop.start().await.unwrap();
        let mut rx = cap_loop.subscribe();
        tokio::time::timeout(Duration::from_secs(2), rx.changed())
            .await
            .expect("a first frame")
            .unwrap();
        let before = cap_loop.frames_captured();

        capturer.renumber("display-2");

        // Re-resolution is rate limited, so allow for that interval plus slack.
        let recovered = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if cap_loop.frames_captured() > before && capturer.rejected.load(Ordering::SeqCst) > 0 {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await;

        cap_loop.stop().await.unwrap();
        recovered.expect("capture should find the renumbered display and resume");
    }

    #[tokio::test]
    async fn an_explicitly_chosen_monitor_is_not_silently_replaced() {
        // Following the display is right by default and wrong when the user named
        // one: capturing a different screen than asked for would leak whatever is on
        // it. Better to keep failing until the chosen display returns.
        let capturer = Arc::new(RenumberingCapturer::new("display-1"));
        let cap_loop = CaptureLoop::new(
            Arc::clone(&capturer) as Arc<dyn ScreenCapturer>,
            CaptureSettings {
                monitor_id: Some("display-1".into()),
                target_fps: 120,
                max_height: 200,
            },
        );

        cap_loop.start().await.unwrap();
        capturer.renumber("display-2");
        let captured = cap_loop.frames_captured();

        tokio::time::sleep(Duration::from_millis(300)).await;
        let after = cap_loop.frames_captured();
        cap_loop.stop().await.unwrap();

        assert_eq!(after, captured, "must not capture a monitor the user did not choose");
        assert!(
            capturer.rejected.load(Ordering::SeqCst) > 0,
            "it should have kept trying the requested display"
        );
    }
}
