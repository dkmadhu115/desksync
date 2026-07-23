//! DeskSync desktop agent entrypoint.
//!
//! Responsibilities:
//! 1. Initialize structured (JSON) tracing.
//! 2. Enforce a single running instance (advisory lock file).
//! 3. Load/persist [`AgentConfig`] and load-or-create the device X25519
//!    identity (the private key never leaves this host).
//! 4. Wire the capture/input subsystems into the [`Agent`] runtime, selecting
//!    the real native backends when built with `--features native`, or the
//!    no-op backends otherwise (headless/CI).
//! 5. Run the capture loop, then stop gracefully on Ctrl-C.
//!
//! The Tauri configuration UI is added in a later phase; the process lifecycle
//! and dependency wiring live here so they are stable from the start.

use std::sync::Arc;

use anyhow::{bail, Context};
use desksync_capture::{CaptureLoop, CaptureSettings, ScreenCapturer};
use desksync_core::subsystem::Subsystem;
use desksync_core::{Agent, AgentConfig, AgentStore, Autostart, SingleInstance};
use desksync_input::InputInjector;

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_env("DESKSYNC_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    fmt().with_env_filter(filter).json().init();
}

/// Load persisted configuration if present; otherwise synthesize one from env
/// overrides/defaults and persist it for next time.
fn load_config(store: &AgentStore) -> AgentConfig {
    if store.config_exists() {
        match store.load_config() {
            Ok(cfg) => return cfg,
            Err(e) => tracing::warn!(error = %e, "failed to read persisted config; using defaults"),
        }
    }
    let cfg = AgentConfig {
        device_id: std::env::var("DESKSYNC_DEVICE_ID").unwrap_or_else(|_| "unregistered".into()),
        backend_url: std::env::var("DESKSYNC_BACKEND_URL")
            .unwrap_or_else(|_| "wss://localhost:8085/api/v1/signaling".into()),
        ..Default::default()
    };
    if let Err(e) = store.save_config(&cfg) {
        tracing::warn!(error = %e, "failed to persist initial config");
    }
    cfg
}

#[cfg(feature = "native")]
fn make_capturer() -> Arc<dyn ScreenCapturer> {
    Arc::new(desksync_capture::XcapCapturer::new())
}

#[cfg(not(feature = "native"))]
fn make_capturer() -> Arc<dyn ScreenCapturer> {
    Arc::new(desksync_capture::NoopCapturer::new())
}

#[cfg(feature = "native")]
fn make_injector() -> Arc<dyn InputInjector> {
    Arc::new(desksync_input::EnigoInjector::new())
}

#[cfg(not(feature = "native"))]
fn make_injector() -> Arc<dyn InputInjector> {
    Arc::new(desksync_input::NoopInjector::new())
}

const BACKEND_KIND: &str = if cfg!(feature = "native") { "native" } else { "noop" };

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let store = AgentStore::platform_default().context("resolving config directory")?;

    // 1) Single-instance guard. Held for the life of the process.
    let _instance =
        match SingleInstance::acquire(store.dir().join("agent.lock")).context("acquiring single-instance lock")? {
            Some(guard) => guard,
            None => bail!("another DeskSync agent instance is already running"),
        };

    // 2) Configuration + device identity.
    let config = load_config(&store);
    config
        .validate()
        .map_err(anyhow::Error::msg)
        .context("invalid agent configuration")?;

    let identity = store.load_or_create_identity().context("loading device identity")?;

    // Reconcile launch-at-login with the configured preference (best-effort).
    if let Ok(autostart) = Autostart::for_current_exe() {
        let result = if config.autostart {
            autostart.enable()
        } else {
            autostart.disable()
        };
        if let Err(e) = result {
            tracing::warn!(error = %e, enabled = config.autostart, "failed to reconcile autostart");
        }
    }

    tracing::info!(
        device_id = %config.device_id,
        backend = %BACKEND_KIND,
        key_fingerprint = %identity.fingerprint(),
        public_key = %identity.public_hex(),
        "desksync agent starting"
    );

    // 3) Build subsystems: the capturer (also validates capture permission on
    // start), the capture loop that drives it, and the input injector.
    let capturer = make_capturer();
    let injector = make_injector();

    let capture_loop = Arc::new(CaptureLoop::new(
        Arc::clone(&capturer),
        CaptureSettings {
            monitor_id: None,
            target_fps: config.target_fps,
            max_height: config.max_height,
        },
    ));

    let subsystems: Vec<Arc<dyn Subsystem>> = vec![capturer, Arc::clone(&capture_loop) as Arc<dyn Subsystem>, injector];

    let agent = Agent::new(config, subsystems);
    agent.start().await.context("failed to start agent")?;

    // Observability: log the first captured frame's dimensions to confirm the
    // pipeline is live.
    {
        let mut frames = capture_loop.subscribe();
        tokio::spawn(async move {
            if frames.changed().await.is_ok() {
                if let Some(frame) = frames.borrow().clone() {
                    tracing::info!(
                        width = frame.width,
                        height = frame.height,
                        "capture pipeline produced first frame"
                    );
                }
            }
        });
    }

    tracing::info!("agent running; press Ctrl-C to stop");
    tokio::signal::ctrl_c()
        .await
        .context("failed to listen for shutdown signal")?;

    agent.stop().await.context("failed to stop agent cleanly")?;
    tracing::info!("agent stopped");
    Ok(())
}
