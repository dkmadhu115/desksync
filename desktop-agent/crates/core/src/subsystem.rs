//! The [`Subsystem`] trait implemented by capture, input, and transport, plus
//! the health model the agent reports to the backend.

use crate::error::Result;
use async_trait::async_trait;

/// Coarse health status aggregated by the agent and surfaced to operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Fully operational.
    Healthy,
    /// Running but impaired (e.g. reduced FPS, relay fallback).
    Degraded,
    /// Not running.
    Stopped,
}

/// A lifecycle-managed component of the agent. Capture, input, and transport
/// all implement this so the [`crate::Agent`] can start/stop them uniformly and
/// remain agnostic of their concrete implementations (dependency inversion).
#[async_trait]
pub trait Subsystem: Send + Sync {
    /// Stable, static name used in logs and errors.
    fn name(&self) -> &'static str;

    /// Start the subsystem. Must be idempotent.
    async fn start(&self) -> Result<()>;

    /// Stop the subsystem. Must be idempotent and must not panic.
    async fn stop(&self) -> Result<()>;

    /// Report current health.
    async fn health(&self) -> HealthStatus;
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::error::AgentError;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Test double that counts start/stop calls and can be made to fail.
    pub struct CountingSubsystem {
        name: &'static str,
        fail_on_start: bool,
        pub starts: AtomicUsize,
        pub stops: AtomicUsize,
    }

    impl CountingSubsystem {
        pub fn new(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                fail_on_start: false,
                starts: AtomicUsize::new(0),
                stops: AtomicUsize::new(0),
            })
        }
        pub fn failing(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                fail_on_start: true,
                starts: AtomicUsize::new(0),
                stops: AtomicUsize::new(0),
            })
        }
    }

    #[async_trait]
    impl Subsystem for CountingSubsystem {
        fn name(&self) -> &'static str {
            self.name
        }
        async fn start(&self) -> Result<()> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if self.fail_on_start {
                return Err(AgentError::subsystem(self.name, "forced failure"));
            }
            Ok(())
        }
        async fn stop(&self) -> Result<()> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn health(&self) -> HealthStatus {
            HealthStatus::Healthy
        }
    }
}
