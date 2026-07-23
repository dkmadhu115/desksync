//! DeskSync desktop agent entrypoint.
//!
//! In Phase 1 this binary:
//! 1. Initializes structured (JSON) tracing.
//! 2. Loads and validates [`AgentConfig`] (currently from defaults/env).
//! 3. Wires the no-op capture/input subsystems into the [`Agent`] runtime.
//! 4. Starts the agent, waits for a shutdown signal, then stops gracefully.
//!
//! The Tauri-based configuration UI and the real capture/encode/stream loop are
//! added in later phases; the process lifecycle and dependency wiring live here
//! so they are stable from the start.

use std::sync::Arc;

use anyhow::Context;
use desksync_capture::NoopCapturer;
use desksync_core::subsystem::Subsystem;
use desksync_core::{Agent, AgentConfig};
use desksync_input::NoopInjector;

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("DESKSYNC_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).json().init();
}

/// Load configuration. Phase 1 uses env overrides on top of defaults; a later
/// phase reads the persisted config file written by the Tauri UI.
fn load_config() -> AgentConfig {
    AgentConfig {
        device_id: std::env::var("DESKSYNC_DEVICE_ID").unwrap_or_else(|_| "unregistered".into()),
        backend_url: std::env::var("DESKSYNC_BACKEND_URL")
            .unwrap_or_else(|_| "wss://localhost:8085/api/v1/signaling".into()),
        ..Default::default()
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let config = load_config();
    config
        .validate()
        .map_err(anyhow::Error::msg)
        .context("invalid agent configuration")?;

    tracing::info!(device_id = %config.device_id, "desksync agent starting");

    let capturer = Arc::new(NoopCapturer::new());
    let injector = Arc::new(NoopInjector::new());
    let subsystems: Vec<Arc<dyn Subsystem>> = vec![capturer, injector];

    let agent = Agent::new(config, subsystems);
    agent.start().await.context("failed to start agent")?;

    tracing::info!("agent running; press Ctrl-C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;

    agent.stop().await.context("failed to stop agent cleanly")?;
    tracing::info!("agent stopped");
    Ok(())
}
