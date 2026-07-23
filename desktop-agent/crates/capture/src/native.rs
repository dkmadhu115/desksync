//! Native screen-capture backend built on `xcap`, which wraps
//! ScreenCaptureKit on macOS, DXGI Desktop Duplication on Windows, and
//! PipeWire/X11 on Linux behind one API.
//!
//! Compiled only when the `native` feature is enabled. All `xcap` calls are
//! blocking, so they run on the Tokio blocking pool to keep the async runtime
//! responsive. Frames are converted from `xcap`'s RGBA to the agent's BGRA
//! [`Frame`] format.

use crate::{Frame, Monitor, ScreenCapturer};
use async_trait::async_trait;
use desksync_core::error::{AgentError, Result};
use desksync_core::subsystem::{HealthStatus, Subsystem};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

fn map_err(context: &str, e: xcap::XCapError) -> AgentError {
    AgentError::subsystem("capture", format!("{context}: {e}"))
}

/// A capturer backed by the platform compositor via `xcap`.
pub struct XcapCapturer {
    running: AtomicBool,
    started: Instant,
}

impl XcapCapturer {
    /// Create a new native capturer.
    pub fn new() -> Self {
        Self {
            running: AtomicBool::new(false),
            started: Instant::now(),
        }
    }

    fn now_us(&self) -> u64 {
        self.started.elapsed().as_micros() as u64
    }
}

impl Default for XcapCapturer {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert an `xcap` monitor handle into the agent's [`Monitor`] descriptor.
fn describe(m: &xcap::Monitor) -> Result<Monitor> {
    Ok(Monitor {
        id: m.id().map_err(|e| map_err("monitor id", e))?.to_string(),
        name: m.name().map_err(|e| map_err("monitor name", e))?,
        width: m.width().map_err(|e| map_err("monitor width", e))?,
        height: m.height().map_err(|e| map_err("monitor height", e))?,
        primary: m.is_primary().map_err(|e| map_err("monitor primary", e))?,
    })
}

#[async_trait]
impl Subsystem for XcapCapturer {
    fn name(&self) -> &'static str {
        "capture"
    }
    async fn start(&self) -> Result<()> {
        // Validate that we can enumerate at least one monitor up front, which
        // also surfaces a missing Screen Recording permission early.
        let monitors = self.monitors().await?;
        if monitors.is_empty() {
            return Err(AgentError::subsystem(
                "capture",
                "no monitors found (is screen-recording permission granted?)",
            ));
        }
        self.running.store(true, Ordering::SeqCst);
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
impl ScreenCapturer for XcapCapturer {
    async fn monitors(&self) -> Result<Vec<Monitor>> {
        tokio::task::spawn_blocking(|| {
            let monitors = xcap::Monitor::all().map_err(|e| map_err("enumerate monitors", e))?;
            monitors.iter().map(describe).collect::<Result<Vec<_>>>()
        })
        .await
        .map_err(|e| AgentError::subsystem("capture", format!("capture task join: {e}")))?
    }

    async fn capture(&self, monitor_id: &str) -> Result<Frame> {
        let target = monitor_id.to_string();
        let timestamp_us = self.now_us();

        tokio::task::spawn_blocking(move || {
            let monitors = xcap::Monitor::all().map_err(|e| map_err("enumerate monitors", e))?;
            let monitor = monitors
                .into_iter()
                .find(|m| matches!(m.id(), Ok(id) if id.to_string() == target))
                .ok_or_else(|| AgentError::subsystem("capture", format!("monitor '{target}' not found")))?;

            let image = monitor.capture_image().map_err(|e| map_err("capture image", e))?;
            let width = image.width();
            let height = image.height();
            let mut data = image.into_raw(); // RGBA8

            // Convert RGBA -> BGRA in place (swap R and B of each pixel).
            for px in data.chunks_exact_mut(4) {
                px.swap(0, 2);
            }

            Ok(Frame {
                width,
                height,
                timestamp_us,
                data,
            })
        })
        .await
        .map_err(|e| AgentError::subsystem("capture", format!("capture task join: {e}")))?
    }
}
