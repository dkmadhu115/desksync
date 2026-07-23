//! Core runtime and shared types for the DeskSync desktop agent.
//!
//! This crate owns:
//! - [`AgentConfig`]: strongly-typed configuration loaded from disk/env.
//! - [`AgentError`]: the unified error type surfaced by the agent.
//! - [`Agent`]: the top-level runtime that wires the capture, input, and
//!   transport subsystems together. Subsystems are injected as trait objects
//!   (dependency inversion) so platform-specific implementations and test
//!   doubles are interchangeable.
//!
//! Phase 1 provides the contracts and a runnable, testable skeleton. The real
//! capture/encode/stream loop is implemented in Phase 3 and Phase 5.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod config;
pub mod error;
pub mod subsystem;

pub use config::AgentConfig;
pub use error::{AgentError, Result};
pub use subsystem::{HealthStatus, Subsystem};

use std::sync::Arc;

/// The top-level agent runtime. It coordinates the lifecycle of every
/// subsystem: start on connect, stop on disconnect, and report health.
pub struct Agent {
    config: AgentConfig,
    subsystems: Vec<Arc<dyn Subsystem>>,
}

impl Agent {
    /// Create a new agent from configuration and its subsystems.
    pub fn new(config: AgentConfig, subsystems: Vec<Arc<dyn Subsystem>>) -> Self {
        Self { config, subsystems }
    }

    /// The device identifier this agent reports to the backend.
    pub fn device_id(&self) -> &str {
        &self.config.device_id
    }

    /// Start every subsystem. If any subsystem fails to start, the already
    /// started subsystems are stopped again to avoid leaving the agent in a
    /// half-initialized state.
    pub async fn start(&self) -> Result<()> {
        tracing::info!(device_id = %self.config.device_id, "agent starting");
        let mut started: Vec<Arc<dyn Subsystem>> = Vec::new();
        for s in &self.subsystems {
            match s.start().await {
                Ok(()) => started.push(Arc::clone(s)),
                Err(e) => {
                    tracing::error!(subsystem = s.name(), error = %e, "subsystem start failed; rolling back");
                    for s in started.iter().rev() {
                        let _ = s.stop().await;
                    }
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    /// Stop every subsystem in reverse order. Errors are logged but do not
    /// abort the shutdown sequence.
    pub async fn stop(&self) -> Result<()> {
        tracing::info!(device_id = %self.config.device_id, "agent stopping");
        for s in self.subsystems.iter().rev() {
            if let Err(e) = s.stop().await {
                tracing::warn!(subsystem = s.name(), error = %e, "subsystem stop error");
            }
        }
        Ok(())
    }

    /// Aggregate health across all subsystems: healthy only if all are healthy.
    pub async fn health(&self) -> HealthStatus {
        for s in &self.subsystems {
            if s.health().await != HealthStatus::Healthy {
                return HealthStatus::Degraded;
            }
        }
        HealthStatus::Healthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subsystem::tests::CountingSubsystem;
    use std::sync::atomic::Ordering;

    fn test_config() -> AgentConfig {
        AgentConfig {
            device_id: "dev-123".into(),
            backend_url: "wss://example.com/signaling".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn start_and_stop_all_subsystems() {
        let a = CountingSubsystem::new("a");
        let b = CountingSubsystem::new("b");
        let agent = Agent::new(
            test_config(),
            vec![
                Arc::clone(&a) as Arc<dyn Subsystem>,
                Arc::clone(&b) as Arc<dyn Subsystem>,
            ],
        );

        agent.start().await.expect("start");
        assert_eq!(a.starts.load(Ordering::SeqCst), 1);
        assert_eq!(b.starts.load(Ordering::SeqCst), 1);
        assert_eq!(agent.health().await, HealthStatus::Healthy);

        agent.stop().await.expect("stop");
        assert_eq!(a.stops.load(Ordering::SeqCst), 1);
        assert_eq!(b.stops.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn start_rolls_back_on_failure() {
        let ok = CountingSubsystem::new("ok");
        let bad = CountingSubsystem::failing("bad");
        let agent = Agent::new(
            test_config(),
            vec![
                Arc::clone(&ok) as Arc<dyn Subsystem>,
                Arc::clone(&bad) as Arc<dyn Subsystem>,
            ],
        );

        assert!(agent.start().await.is_err());
        // ok started then was rolled back (stopped).
        assert_eq!(ok.starts.load(Ordering::SeqCst), 1);
        assert_eq!(ok.stops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn device_id_accessor() {
        let agent = Agent::new(test_config(), vec![]);
        assert_eq!(agent.device_id(), "dev-123");
    }
}
