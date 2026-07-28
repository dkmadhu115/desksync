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
use desksync_backend::{render_qr, AuthSession, BackendApi, BackendClient, Credentials};
use desksync_capture::{CaptureLoop, CaptureSettings, ScreenCapturer};
use desksync_core::identity::DeviceIdentity;
use desksync_core::subsystem::Subsystem;
use desksync_core::{
    clear_tokens, default_secret_store, save_tokens, Activation, Agent, AgentConfig, AgentStore, ServiceManager,
    SingleInstance, TokenBundle,
};
use desksync_devtools::{DevToolsService, SshHost, SshHostRegistry, TokioCommandRunner, Workspace, WorkspaceRegistry};
use desksync_input::{Clipboard, InputInjector, InputRouter};
use desksync_ipc::{Request as IpcRequest, Response as IpcResponse, StatusSource};

mod agent_auth;
mod service_state;
#[cfg(feature = "native")]
mod session_runtime;
mod setup;

use service_state::ServiceState;

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

/// Initiate a pairing for this desktop, printing a scannable QR code and the
/// manual fallback code. Runs without the single-instance lock so it can be used
/// while the daemon is running.
///
/// Uses stored credentials when available (so no environment variables are
/// needed) and registers the desktop first if it isn't registered yet.
async fn run_pairing(store: &AgentStore, config: &AgentConfig, identity: &DeviceIdentity) -> anyhow::Result<()> {
    let Some(agent) = agent_auth::bootstrap(store, config, identity).await? else {
        bail!("not signed in — run `desksync-agent login` first");
    };

    let challenge = agent
        .session
        .initiate_pairing(&agent.device_id)
        .await
        .context("initiating pairing")?;

    let qr = render_qr(&challenge.qr_payload).context("rendering pairing QR")?;
    println!("\nScan this QR code with the DeskSync mobile app:\n\n{qr}");
    println!("Or enter the pairing details manually:");
    println!("  Pairing ID: {}", challenge.pairing_id);
    println!("  Code:       {}", challenge.manual_code);
    if !challenge.expires_at.is_empty() {
        println!("  Expires at: {}", challenge.expires_at);
    }
    println!("\nRegistered device id: {}\n", agent.device_id);
    Ok(())
}

/// Authenticate and persist the resulting tokens (+ current device id) to the OS
/// credential store so subsequent runs don't need credentials in the environment.
///
/// Sign-in goes through the system browser (Google) by default, which is the
/// path a real user takes. `login --password` keeps the email/password flow for
/// CI and headless boxes, reading `DESKSYNC_EMAIL`/`DESKSYNC_PASSWORD`.
async fn run_login(
    store: &AgentStore,
    config: &AgentConfig,
    identity: &DeviceIdentity,
    mode: LoginMode,
) -> anyhow::Result<()> {
    let tokens = match mode {
        LoginMode::Browser => desksync_backend::google_login(&config.api_url)
            .await
            .context("browser sign-in failed")?,
        LoginMode::Password => {
            let creds = Credentials::from_env()?;
            let client = BackendClient::new(&config.api_url).context("building backend client")?;
            client
                .login(&creds.email, &creds.password)
                .await
                .context("login failed")?
        }
    };

    let secrets = default_secret_store(store.dir());
    let bundle = TokenBundle {
        access_token: tokens.access_token,
        refresh_token: tokens.refresh_token,
        device_id: config.device_id.clone(),
    };
    save_tokens(secrets.as_ref(), &bundle).context("persisting credentials")?;
    println!(
        "Signed in. Credentials stored securely in {}.",
        agent_auth::secret_backend_label()
    );

    // Sign-in is also enrollment: register the desktop now so it shows up in the
    // mobile app immediately instead of on the next daemon start.
    match agent_auth::bootstrap(store, config, identity).await {
        Ok(Some(agent)) => println!("This desktop is registered as device {}.", agent.device_id),
        Ok(None) => {}
        Err(e) => println!("Signed in, but registering this desktop failed: {e:#}"),
    }
    Ok(())
}

/// How `login` should authenticate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoginMode {
    /// Google sign-in via the system browser (default).
    Browser,
    /// Email/password from the environment (CI/headless).
    Password,
}

/// Remove any persisted credentials from the secret store.
fn run_logout(store: &AgentStore) -> anyhow::Result<()> {
    let secrets = default_secret_store(store.dir());
    clear_tokens(secrets.as_ref()).context("clearing credentials")?;
    println!(
        "Signed out. Stored credentials removed from {}.",
        agent_auth::secret_backend_label()
    );
    Ok(())
}

/// Install, remove, or inspect the background service.
///
/// Installing makes the agent behave like a product rather than a terminal
/// command: it survives closing the shell, restarts if it crashes, comes back at
/// login, and writes its logs to a fixed path.
fn run_service(action: Option<&str>) -> anyhow::Result<()> {
    let manager = ServiceManager::for_current_exe().context("resolving service paths")?;

    match action {
        Some("install") => {
            let activation = manager.install().context("installing the background service")?;
            match activation {
                Activation::Started => println!("Background service installed and running."),
                Activation::PendingLogin => {
                    println!("Background service installed. It starts automatically at your next login.");
                }
            }
            println!("  Service entry: {}", manager.entry_path().display());
            if let Some(log) = manager.log_path() {
                println!("  Logs:          {}", log.display());
            }
            // The installed entry points at this exact binary, and macOS ties
            // screen-recording consent to the binary itself — so a rebuilt or
            // moved executable needs a re-install and a fresh permission grant.
            println!(
                "  Executable:    {}",
                std::env::current_exe()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| "unknown".into())
            );
            println!("\nRe-run `service install` after replacing the executable.");
        }
        Some("uninstall") => {
            manager.uninstall().context("removing the background service")?;
            println!("Background service stopped and removed.");
        }
        Some("restart") => {
            manager.restart().context("restarting the background service")?;
            println!("Background service restarted.");
        }
        Some("status") => {
            let status = manager.status();
            println!("Installed: {}", if status.installed { "yes" } else { "no" });
            match status.running {
                Some(true) => println!("Running:   yes"),
                Some(false) => println!("Running:   no"),
                None => println!("Running:   unknown (starts at login on this platform)"),
            }
            println!("Entry:     {}", status.entry_path.display());
            if let Some(log) = manager.log_path() {
                println!("Logs:      {}", log.display());
            }
        }
        _ => bail!("usage: desksync-agent service <install|uninstall|restart|status>"),
    }
    Ok(())
}

/// Ask the running service what it is doing, over local IPC.
///
/// This is the "why isn't it working" command: it reports sign-in, the device it
/// is acting as, whether capture is actually producing frames, how many sessions
/// are live, and the last error — without needing the log file.
async fn run_status(store: &AgentStore) -> anyhow::Result<()> {
    match desksync_ipc::request(store.dir(), IpcRequest::GetStatus).await {
        Ok(IpcResponse::Status(status)) => {
            println!("DeskSync service v{}", status.version);
            let sign_in = match (status.signed_in, status.signing_in) {
                (true, _) => "yes",
                (false, true) => "signing in…",
                (false, false) => "no",
            };
            println!("  Signed in:       {sign_in}");
            println!("  Device id:       {}", status.device_id);
            println!("  Backend:         {}", status.api_url);
            println!(
                "  Capture:         max {}p at {} fps — {}",
                status.capture.max_height,
                status.capture.target_fps,
                if status.capture.producing_frames {
                    "producing frames"
                } else {
                    "NO frames captured"
                }
            );
            println!("  Active sessions: {}", status.active_sessions);
            println!("  Uptime:          {}s", status.uptime_secs);
            match &status.last_error {
                Some(e) => println!("  Last error:      {e}"),
                None => println!("  Last error:      none"),
            }

            if !status.permissions.is_empty() {
                println!("  Permissions:");
                for p in &status.permissions {
                    let state = match p.granted {
                        Some(true) => "granted",
                        Some(false) => "NOT granted",
                        None => "unknown",
                    };
                    println!("    {:<34} {state}", p.name);
                    if p.granted == Some(false) {
                        println!("    {:<34} without it, {}", "", p.consequence);
                    }
                }
            }

            // Frames stop at the capture backend, so a missing grant shows up here
            // long before anything logs an error.
            if !status.capture.producing_frames {
                println!("\nNo frames captured. Run `desksync-agent permissions` to check access.");
            }
            Ok(())
        }
        Ok(IpcResponse::Error { message }) => bail!("service reported: {message}"),
        Ok(other) => bail!("unexpected response from the service: {other:?}"),
        Err(desksync_ipc::IpcError::NotRunning) => {
            println!("The DeskSync service is not running.");
            println!("Start it in the background with `desksync-agent service install`,");
            println!("or in the foreground with `desksync-agent`.");
            Ok(())
        }
        Err(e) => Err(anyhow::Error::new(e).context("querying the service")),
    }
}

/// Print the command surface. Kept short: the daemon is the default mode and the
/// rest are one-shot administrative commands.
fn print_usage() {
    println!(
        "DeskSync desktop agent\n\n\
         Usage:\n\
         \x20 desksync-agent                    run the agent in the foreground\n\
         \x20 desksync-agent setup              guided first-run setup (start here)\n\
         \x20 desksync-agent login              sign in with Google via your browser\n\
         \x20 desksync-agent login --password   sign in with DESKSYNC_EMAIL/DESKSYNC_PASSWORD\n\
         \x20 desksync-agent logout             remove stored credentials\n\
         \x20 desksync-agent pair               show a pairing QR code for the mobile app\n\
         \x20 desksync-agent status             ask the running agent what it is doing\n\
         \x20 desksync-agent permissions        show which OS permissions are granted\n\
         \x20 desksync-agent service install    run in the background, starting at login\n\
         \x20 desksync-agent service status     show whether the service is installed/running\n\
         \x20 desksync-agent service restart    restart the background service\n\
         \x20 desksync-agent service uninstall  stop and remove the background service\n"
    );
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
/// periodic heartbeats. Token rotation is handled by the [`AuthSession`], so a
/// failure here is transient (network/backend) and simply retried next tick.
fn spawn_heartbeat(session: Arc<AuthSession>, device_id: String, interval_secs: u64, state: Arc<ServiceState>) {
    let interval = interval_secs.max(5);
    tokio::spawn(async move {
        tracing::info!(device_id = %device_id, interval_secs = interval, "reporting presence");
        let mut ticker = tokio::time::interval(Duration::from_secs(interval));
        // interval() fires immediately on the first tick, so presence is
        // reported as soon as the daemon is up.
        loop {
            ticker.tick().await;
            match session.heartbeat(&device_id).await {
                // Clearing on success is what makes `status` show recovery rather
                // than a stale complaint from hours ago.
                Ok(()) => state.clear_error(),
                Err(e) => {
                    tracing::warn!(error = %e, "heartbeat failed; will retry");
                    state.record_error(format!("heartbeat failed: {e}"));
                }
            }
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

    // One-shot subcommands run before the single-instance lock so they can be
    // used while the daemon is active. Running with no arguments starts the
    // daemon; an unrecognized argument is an error rather than a silent daemon
    // start, which would otherwise swallow typos like `serivce install`.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("pair") => return run_pairing(&store, &config, &identity).await,
        Some("login") => {
            let mode = if args.iter().any(|a| a == "--password") {
                LoginMode::Password
            } else {
                LoginMode::Browser
            };
            return run_login(&store, &config, &identity, mode).await;
        }
        Some("logout") => return run_logout(&store),
        Some("setup") => return setup::run(&store, &config, &identity).await,
        Some("permissions") => return setup::print_permissions(),
        Some("status") => return run_status(&store).await,
        Some("service") => return run_service(args.get(1).map(String::as_str)),
        Some("help" | "--help" | "-h") => {
            print_usage();
            return Ok(());
        }
        Some(other) => {
            print_usage();
            bail!("unknown command `{other}`");
        }
        None => {}
    }

    // Single-instance guard for the daemon. Held for the life of the process.
    let _instance =
        match SingleInstance::acquire(store.dir().join("agent.lock")).context("acquiring single-instance lock")? {
            Some(guard) => guard,
            None => bail!("another DeskSync agent instance is already running"),
        };

    // Reconcile launch-at-login with the configured preference (best-effort).
    // Entry-only: the service must not start or stop itself here.
    if let Ok(service) = ServiceManager::for_current_exe() {
        if let Err(e) = service.reconcile_entry(config.autostart) {
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

    // Live state published over local IPC, so `desksync-agent status` can answer
    // "what are you doing?" once this process has no terminal attached.
    let state = Arc::new(ServiceState::new(
        &config,
        Arc::clone(&capture_loop),
        ServiceManager::for_current_exe()
            .ok()
            .and_then(|m| m.log_path())
            .map(|p| p.display().to_string()),
    ));

    // Serve status queries *before* signing in. Sign-in reads the OS keychain,
    // which can block for a long time (macOS asks for access after any rebuild),
    // and that is exactly when someone runs `status` to see what is happening.
    // A failure here must not stop the agent: losing diagnostics is far less bad
    // than not running at all.
    let _ipc = match desksync_ipc::listen(store.dir(), Arc::clone(&state) as Arc<dyn StatusSource>).await {
        Ok(server) => Some(server),
        Err(e) => {
            tracing::warn!(error = %e, "status ipc unavailable; `desksync-agent status` will not work");
            None
        }
    };

    // Sign-in state: stored keychain credentials (from `desksync-agent login`),
    // or password credentials in the environment for CI. With neither, the agent
    // still runs locally — it just can't report presence or accept sessions,
    // which is recoverable by signing in and restarting.
    let account = match agent_auth::bootstrap(&store, &config, &identity).await {
        Ok(Some(account)) => {
            state.set_signed_in(&account.device_id);
            Some(account)
        }
        Ok(None) => {
            let msg = "not signed in; run `desksync-agent login` to connect this desktop to your account";
            tracing::warn!("{msg}");
            state.set_signed_out();
            state.record_error(msg);
            None
        }
        Err(e) => {
            let msg = format!("{e:#}");
            tracing::error!(error = %msg, "sign-in failed; continuing without backend connectivity");
            state.set_signed_out();
            state.record_error(msg);
            None
        }
    };

    // Keep the device marked "online" in the backend while the daemon runs.
    if let Some(account) = &account {
        spawn_heartbeat(
            Arc::clone(&account.session),
            account.device_id.clone(),
            config.heartbeat_secs,
            Arc::clone(&state),
        );
    }

    // Serve incoming remote-control sessions: discover them from the backend,
    // answer over WebRTC, stream the screen, and route input/control frames.
    #[cfg(feature = "native")]
    {
        match &account {
            Some(account) => {
                let manager = Arc::new(session_runtime::SessionManager::new(
                    Arc::clone(&account.session),
                    account.device_id.clone(),
                    Arc::clone(&capture_loop),
                    Arc::clone(&input_router),
                    Arc::clone(&devtools),
                    Arc::clone(&state),
                ));
                tokio::spawn(manager.run());
            }
            None => tracing::warn!("incoming remote sessions are disabled until you sign in"),
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
