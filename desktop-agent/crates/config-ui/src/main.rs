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
use std::time::Duration;

use anyhow::{bail, Context};
use desksync_backend::{
    detect_device_name, detect_platform, render_qr, BackendApi, BackendClient, Credentials, DeviceProfile,
    Enrollment,
};
use desksync_capture::{CaptureLoop, CaptureSettings, ScreenCapturer};
use desksync_core::identity::DeviceIdentity;
use desksync_core::subsystem::Subsystem;
use desksync_core::{Agent, AgentConfig, AgentStore, Autostart, SingleInstance};
use desksync_devtools::{DevToolsService, SshHost, SshHostRegistry, TokioCommandRunner, Workspace, WorkspaceRegistry};
use desksync_input::{Clipboard, InputInjector, InputRouter};

#[cfg(feature = "native")]
mod session_runtime;

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

#[cfg(feature = "native")]
fn make_clipboard() -> Arc<dyn Clipboard> {
    Arc::new(desksync_input::clipboard::ArboardClipboard::new())
}

#[cfg(not(feature = "native"))]
fn make_clipboard() -> Arc<dyn Clipboard> {
    Arc::new(desksync_input::NoopClipboard::new())
}

const BACKEND_KIND: &str = if cfg!(feature = "native") { "native" } else { "noop" };

/// Enroll this desktop and initiate a pairing, printing a scannable QR code and
/// the manual fallback code. Runs without the single-instance lock so it can be
/// used while the daemon is running. Credentials come from
/// `DESKSYNC_EMAIL`/`DESKSYNC_PASSWORD`; the REST base URL from `config.api_url`.
async fn run_pairing(store: &AgentStore, config: &AgentConfig, identity: &DeviceIdentity) -> anyhow::Result<()> {
    let creds = Credentials::from_env()?;
    let client = BackendClient::new(&config.api_url).context("building backend client")?;
    let enrollment = Enrollment::new(Arc::new(client));

    let profile = DeviceProfile {
        platform: detect_platform(),
        name: detect_device_name(),
        public_key: identity.public_base64(),
    };

    let outcome = enrollment.run(&creds, profile).await.context("enrollment failed")?;

    // Persist the server-assigned device id so subsequent runs reuse it.
    let mut updated = config.clone();
    updated.device_id = outcome.device_id.clone();
    if let Err(e) = store.save_config(&updated) {
        tracing::warn!(error = %e, "failed to persist device id after pairing");
    }

    let qr = render_qr(&outcome.challenge.qr_payload).context("rendering pairing QR")?;
    println!("\nScan this QR code with the DeskSync mobile app:\n\n{qr}");
    println!("Or enter the pairing details manually:");
    println!("  Pairing ID: {}", outcome.challenge.pairing_id);
    println!("  Code:       {}", outcome.challenge.manual_code);
    if !outcome.challenge.expires_at.is_empty() {
        println!("  Expires at: {}", outcome.challenge.expires_at);
    }
    println!("\nRegistered device id: {}\n", outcome.device_id);
    Ok(())
}

/// Load a JSON array of registry items from `<config-dir>/<file>`, returning an
/// empty list when the file is absent and logging (but tolerating) parse errors.
fn load_workspaces(store: &AgentStore) -> Vec<Workspace> {
    let path = store.dir().join("workspaces.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str::<Vec<Workspace>>(&s).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ignoring invalid workspaces.json");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

fn load_ssh_hosts(store: &AgentStore) -> Vec<SshHost> {
    let path = store.dir().join("ssh_hosts.json");
    match std::fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str::<Vec<SshHost>>(&s).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "ignoring invalid ssh_hosts.json");
            Vec::new()
        }),
        Err(_) => Vec::new(),
    }
}

/// Build the developer-tools service from persisted registries. Invalid entries
/// fail closed to an empty registry so a bad config never widens the allowlist.
/// The native WebRTC control channel dispatches `dev_action` frames to
/// `DevToolsService::handle_frame` (wired with the media peer).
fn build_devtools(store: &AgentStore) -> DevToolsService {
    let workspaces = WorkspaceRegistry::from_items(load_workspaces(store)).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "rejecting workspaces registry; starting empty");
        WorkspaceRegistry::new()
    });
    let hosts = SshHostRegistry::from_items(load_ssh_hosts(store)).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "rejecting ssh hosts registry; starting empty");
        SshHostRegistry::new()
    });
    DevToolsService::new(
        workspaces,
        hosts,
        Arc::new(TokioCommandRunner::default()),
        std::env::consts::OS,
    )
}

/// Spawn a background task that keeps this device marked "online" by sending
/// periodic heartbeats to the backend. Credentials come from
/// `DESKSYNC_EMAIL`/`DESKSYNC_PASSWORD`; the device id and REST base URL from
/// the persisted config. If the device is not yet paired or credentials are
/// absent, heartbeats are skipped (the device will simply show offline).
fn spawn_heartbeat(config: &AgentConfig) {
    let device_id = config.device_id.clone();
    if device_id.trim().is_empty() || device_id == "unregistered" {
        tracing::warn!("device not registered yet; skipping heartbeats (run `pair` first)");
        return;
    }
    let creds = match Credentials::from_env() {
        Ok(c) => c,
        Err(_) => {
            tracing::warn!(
                "DESKSYNC_EMAIL/DESKSYNC_PASSWORD not set; skipping heartbeats (device will show offline)"
            );
            return;
        }
    };
    let api_url = config.api_url.clone();
    let interval = config.heartbeat_secs.max(5);

    tokio::spawn(async move {
        let client = match BackendClient::new(&api_url) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "heartbeat: failed to build backend client");
                return;
            }
        };
        // Authenticate once up front; refresh/re-login on demand below.
        let mut tokens = match client.login(&creds.email, &creds.password).await {
            Ok(t) => {
                tracing::info!(device_id = %device_id, "heartbeat: authenticated; reporting presence");
                t
            }
            Err(e) => {
                tracing::error!(error = %e, "heartbeat: initial login failed");
                return;
            }
        };

        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        // Send an immediate first heartbeat, then on each tick.
        loop {
            if client.heartbeat(&tokens.access_token, &device_id).await.is_err() {
                // Access token likely expired: rotate the refresh token, or
                // fall back to a full re-login, then retry once.
                let refreshed = match client.refresh(&tokens.refresh_token).await {
                    Ok(t) => Some(t),
                    Err(_) => client.login(&creds.email, &creds.password).await.ok(),
                };
                match refreshed {
                    Some(t) => {
                        tokens = t;
                        if let Err(e) = client.heartbeat(&tokens.access_token, &device_id).await {
                            tracing::warn!(error = %e, "heartbeat failed after re-auth; will retry");
                        }
                    }
                    None => tracing::warn!("heartbeat: re-authentication failed; will retry"),
                }
            }
            ticker.tick().await;
        }
    });
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_tracing();

    let store = AgentStore::platform_default().context("resolving config directory")?;

    // Configuration + device identity are needed for every mode.
    let config = load_config(&store);
    config
        .validate()
        .map_err(anyhow::Error::msg)
        .context("invalid agent configuration")?;

    let identity = store.load_or_create_identity().context("loading device identity")?;

    // `desksync-agent pair` runs the enrollment + pairing-initiation flow and
    // exits. It does not take the single-instance lock so it can be run while
    // the daemon is active.
    if std::env::args().nth(1).as_deref() == Some("pair") {
        return run_pairing(&store, &config, &identity).await;
    }

    // Single-instance guard for the daemon. Held for the life of the process.
    let _instance =
        match SingleInstance::acquire(store.dir().join("agent.lock")).context("acquiring single-instance lock")? {
            Some(guard) => guard,
            None => bail!("another DeskSync agent instance is already running"),
        };

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

    // Input requires OS permission (Accessibility on macOS). Start it here and
    // treat failure as non-fatal so the agent still streams (view-only) and
    // stays online; grant the permission and restart to enable remote control.
    match injector.start().await {
        Ok(()) => tracing::info!("input backend ready"),
        Err(e) => tracing::warn!(
            error = %e,
            "input disabled (view-only mode); grant Accessibility permission and restart to enable remote control"
        ),
    }

    let subsystems: Vec<Arc<dyn Subsystem>> = vec![
        Arc::clone(&capturer) as Arc<dyn Subsystem>,
        Arc::clone(&capture_loop) as Arc<dyn Subsystem>,
    ];

    // Developer quick-launch/shortcut engine. Loaded and validated at startup
    // (fail-closed on bad config) so it is ready for the control channel that
    // the native WebRTC peer wires to `DevToolsService::handle_frame`.
    let devtools = Arc::new(build_devtools(&store));
    tracing::info!(
        workspaces = devtools.workspaces().list().len(),
        ssh_hosts = devtools.hosts().list().len(),
        "developer tools engine ready"
    );

    // Router that dispatches inbound input frames (from the mobile) to the OS
    // injector + clipboard. Shares the same injector started above so it uses
    // the (possibly permission-degraded) native backend.
    let input_router = Arc::new(InputRouter::new(Arc::clone(&injector), make_clipboard()));

    // Keep the device marked "online" in the backend while the daemon runs.
    spawn_heartbeat(&config);

    // Serve incoming remote-control sessions: discover them from the backend,
    // answer over WebRTC, stream the screen, and route input/control frames.
    #[cfg(feature = "native")]
    {
        match Credentials::from_env() {
            Ok(creds) => {
                let manager = Arc::new(session_runtime::SessionManager::new(
                    config.api_url.clone(),
                    config.device_id.clone(),
                    creds,
                    Arc::clone(&capture_loop),
                    Arc::clone(&input_router),
                    Arc::clone(&devtools),
                ));
                tokio::spawn(manager.run());
            }
            Err(_) => tracing::warn!(
                "DESKSYNC_EMAIL/DESKSYNC_PASSWORD not set; incoming remote sessions are disabled"
            ),
        }
    }
    #[cfg(not(feature = "native"))]
    {
        let _ = (&input_router, &devtools);
    }

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
    let _ = injector.stop().await;
    tracing::info!("agent stopped");
    Ok(())
}
