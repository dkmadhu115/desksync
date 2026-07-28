//! Live runtime state, published over local IPC.
//!
//! Once the agent runs in the background there is no terminal to watch, so it has
//! to be able to answer "what are you doing?" on demand. This is the shared,
//! cheap-to-update state behind `desksync-agent status`.
//!
//! Everything here is written from concurrent tasks (heartbeat, session runtime)
//! and read by IPC connections, so each field uses the lightest synchronization
//! that fits: atomics for counters and flags, a lock only for the two strings.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use async_trait::async_trait;
use desksync_capture::CaptureLoop;
use desksync_core::AgentConfig;
use desksync_ipc::{CaptureStatus, ServiceStatus, StatusSource};

/// Shared state describing what the service is currently doing.
pub struct ServiceState {
    started: Instant,
    signed_in: AtomicBool,
    device_id: RwLock<String>,
    api_url: String,
    target_fps: u32,
    max_height: u32,
    capture: Arc<CaptureLoop>,
    active_sessions: AtomicU32,
    last_error: RwLock<Option<String>>,
    log_path: Option<String>,
}

impl ServiceState {
    /// Build the state for a running service.
    pub fn new(config: &AgentConfig, capture: Arc<CaptureLoop>, log_path: Option<String>) -> Self {
        Self {
            started: Instant::now(),
            signed_in: AtomicBool::new(false),
            device_id: RwLock::new(config.device_id.clone()),
            api_url: config.api_url.clone(),
            target_fps: config.target_fps,
            max_height: config.max_height,
            capture,
            active_sessions: AtomicU32::new(0),
            last_error: RwLock::new(None),
            log_path,
        }
    }

    /// Record that usable credentials were found, along with the device this
    /// service is acting as.
    pub fn set_signed_in(&self, device_id: &str) {
        self.signed_in.store(true, Ordering::Relaxed);
        *self.device_id.write().expect("device id lock poisoned") = device_id.to_string();
    }

    /// Record the most recent problem worth surfacing to the user.
    pub fn record_error(&self, error: impl std::fmt::Display) {
        *self.last_error.write().expect("last error lock poisoned") = Some(error.to_string());
    }

    /// Clear the last error after a successful operation, so `status` reflects
    /// recovery instead of a stale complaint.
    pub fn clear_error(&self) {
        *self.last_error.write().expect("last error lock poisoned") = None;
    }

    /// Count a session for as long as the returned guard is alive.
    ///
    /// Only the native session runtime serves sessions, so this is unused in the
    /// headless build.
    #[cfg_attr(not(feature = "native"), allow(dead_code))]
    pub fn track_session(self: &Arc<Self>) -> SessionGuard {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
        SessionGuard {
            state: Arc::clone(self),
        }
    }

    /// Sessions currently being served.
    pub fn active_sessions(&self) -> u32 {
        self.active_sessions.load(Ordering::Relaxed)
    }
}

/// Decrements the active-session count when dropped.
///
/// A guard rather than explicit decrements: a session task can end by error,
/// cancellation, or normal completion, and all three must be counted the same.
#[cfg_attr(not(feature = "native"), allow(dead_code))]
pub struct SessionGuard {
    state: Arc<ServiceState>,
}

impl Drop for SessionGuard {
    fn drop(&mut self) {
        self.state.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }
}

#[async_trait]
impl StatusSource for ServiceState {
    async fn status(&self) -> ServiceStatus {
        ServiceStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_secs: self.started.elapsed().as_secs(),
            signed_in: self.signed_in.load(Ordering::Relaxed),
            device_id: self.device_id.read().expect("device id lock poisoned").clone(),
            api_url: self.api_url.clone(),
            capture: CaptureStatus {
                target_fps: self.target_fps,
                max_height: self.max_height,
                // Read live rather than latched: a capture backend that stops
                // producing (permission revoked, display asleep) should show as
                // not producing.
                producing_frames: self.capture.subscribe().borrow().is_some(),
            },
            active_sessions: self.active_sessions(),
            last_error: self.last_error.read().expect("last error lock poisoned").clone(),
        }
    }

    fn log_path(&self) -> Option<String> {
        self.log_path.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desksync_capture::{CaptureSettings, NoopCapturer};

    fn state() -> Arc<ServiceState> {
        let config = AgentConfig {
            target_fps: 20,
            max_height: 720,
            ..AgentConfig::default()
        };
        let capture = Arc::new(CaptureLoop::new(
            Arc::new(NoopCapturer::new()),
            CaptureSettings {
                monitor_id: None,
                target_fps: config.target_fps,
                max_height: config.max_height,
            },
        ));
        Arc::new(ServiceState::new(&config, capture, Some("/tmp/agent.log".into())))
    }

    #[tokio::test]
    async fn a_fresh_service_reports_signed_out_with_no_sessions() {
        let status = state().status().await;
        assert!(!status.signed_in);
        assert_eq!(status.active_sessions, 0);
        assert!(status.last_error.is_none());
        assert!(!status.capture.producing_frames, "no frames captured yet");
    }

    #[tokio::test]
    async fn signing_in_publishes_the_device_id() {
        let state = state();
        state.set_signed_in("device-7");

        let status = state.status().await;
        assert!(status.signed_in);
        assert_eq!(status.device_id, "device-7");
    }

    #[tokio::test]
    async fn errors_are_reported_then_cleared_on_recovery() {
        let state = state();
        state.record_error("heartbeat failed");
        assert_eq!(state.status().await.last_error.as_deref(), Some("heartbeat failed"));

        state.clear_error();
        assert!(state.status().await.last_error.is_none());
    }

    #[tokio::test]
    async fn sessions_are_counted_while_their_guard_lives() {
        let state = state();
        {
            let _one = state.track_session();
            let _two = state.track_session();
            assert_eq!(state.status().await.active_sessions, 2);
        }
        // Both guards dropped: a session that ends by error still stops counting.
        assert_eq!(state.status().await.active_sessions, 0);
    }

    #[tokio::test]
    async fn status_reports_the_configured_capture_settings_and_log_path() {
        let state = state();
        let status = state.status().await;
        assert_eq!(status.capture.target_fps, 20);
        assert_eq!(status.capture.max_height, 720);
        assert_eq!(state.log_path().as_deref(), Some("/tmp/agent.log"));
    }
}
