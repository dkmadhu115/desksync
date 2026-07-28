//! Installing the agent as a background service.
//!
//! [`Autostart`] answers "start at login"; this module answers "run it *now*,
//! keep it running, and put its logs somewhere I can find them". That is the
//! difference between a binary a developer launches in a terminal and something
//! a normal user installs once and forgets.
//!
//! Platform support mirrors how each OS wants per-user background work done:
//! - **macOS** — a launchd LaunchAgent, activated immediately with `launchctl`,
//!   restarted automatically by launchd (`KeepAlive`).
//! - **Linux/Windows** — the XDG autostart entry / Startup shortcut is written
//!   and takes effect at next login; [`ServiceManager::install`] reports that
//!   rather than pretending the service is already up.
//!
//! Logs always go to a stable per-user location ([`ServiceManager::log_path`]),
//! because the first question about a background service is always "why isn't it
//! working".

use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

use crate::autostart::Autostart;
#[cfg(target_os = "macos")]
use crate::autostart::LABEL;
use crate::error::{AgentError, Result};

/// What happened when the service was installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// The service was started and is running now.
    Started,
    /// The entry was written; it starts at the next login.
    PendingLogin,
}

/// Whether the service is installed and, if the platform can tell, running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceStatus {
    /// The autostart/service entry exists on disk.
    pub installed: bool,
    /// `Some(true)`/`Some(false)` when the platform can report liveness,
    /// `None` when installation is all we can observe.
    pub running: Option<bool>,
    /// Path to the entry, for display in `status` output.
    pub entry_path: PathBuf,
}

/// Installs, removes, and inspects the agent's background service.
#[derive(Debug, Clone)]
pub struct ServiceManager {
    autostart: Autostart,
}

impl ServiceManager {
    /// Build a manager for the currently running executable.
    ///
    /// The executable path is baked into the service entry, so install from the
    /// location you intend to keep: on macOS, screen-recording consent is tied
    /// to the binary, and moving or replacing it re-prompts.
    pub fn for_current_exe() -> Result<Self> {
        let logs = default_log_dir()?;
        Ok(Self {
            autostart: Autostart::for_current_exe()?.with_logs(logs),
        })
    }

    /// Build a manager over an explicit autostart configuration (used in tests).
    pub fn with_autostart(autostart: Autostart) -> Self {
        Self { autostart }
    }

    /// Where the service writes stdout/stderr.
    pub fn log_path(&self) -> Option<PathBuf> {
        self.autostart.log_path()
    }

    /// Path to the platform service entry.
    pub fn entry_path(&self) -> PathBuf {
        self.autostart.entry_path()
    }

    /// Write the service entry and start the service if the platform allows it.
    /// Idempotent: re-installing refreshes the entry and restarts the service.
    pub fn install(&self) -> Result<Activation> {
        self.autostart.enable()?;
        self.activate()
    }

    /// Stop the service and remove its entry. Idempotent.
    pub fn uninstall(&self) -> Result<()> {
        // Deactivate first: on macOS the plist path is the handle used to unload
        // the job, so removing the file first would orphan a running service.
        let _ = self.deactivate();
        self.autostart.disable()
    }

    /// Stop and start the service, picking up a new binary or config.
    pub fn restart(&self) -> Result<Activation> {
        if !self.autostart.is_enabled() {
            return Err(AgentError::Config(
                "service is not installed; run `desksync-agent service install` first".into(),
            ));
        }
        let _ = self.deactivate();
        self.activate()
    }

    /// Reconcile only the on-disk entry with a "start at login" preference,
    /// without starting or stopping anything.
    ///
    /// This is what the running daemon uses: it must keep the entry pointing at
    /// the current executable (with log redirection intact) but must never
    /// activate or deactivate the service, since that would mean restarting or
    /// killing itself mid-startup.
    pub fn reconcile_entry(&self, start_at_login: bool) -> Result<()> {
        if start_at_login {
            self.autostart.enable()
        } else {
            self.autostart.disable()
        }
    }

    /// Report installation (and liveness where the platform can tell).
    pub fn status(&self) -> ServiceStatus {
        ServiceStatus {
            installed: self.autostart.is_enabled(),
            running: self.is_running(),
            entry_path: self.entry_path(),
        }
    }

    // ---- platform activation ----

    #[cfg(target_os = "macos")]
    fn activate(&self) -> Result<Activation> {
        let plist = self.entry_path();
        let domain = gui_domain()?;
        // `bootstrap` is the modern (10.11+) way in; `load -w` is the fallback
        // for older systems and for domains where bootstrap is unavailable.
        if run_ok("launchctl", &["bootstrap", &domain, &plist.to_string_lossy()])
            || run_ok("launchctl", &["load", "-w", &plist.to_string_lossy()])
        {
            return Ok(Activation::Started);
        }
        Err(AgentError::Config(format!(
            "failed to start the service; try `launchctl bootstrap {domain} {}`",
            plist.display()
        )))
    }

    #[cfg(target_os = "macos")]
    fn deactivate(&self) -> Result<()> {
        let plist = self.entry_path();
        let domain = gui_domain()?;
        // Either form succeeding is enough, and "was not running" is also fine:
        // deactivate exists to guarantee the service is stopped afterwards.
        let _ = run_ok("launchctl", &["bootout", &format!("{domain}/{LABEL}")])
            || run_ok("launchctl", &["unload", "-w", &plist.to_string_lossy()]);
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn is_running(&self) -> Option<bool> {
        let domain = gui_domain().ok()?;
        Some(run_ok("launchctl", &["print", &format!("{domain}/{LABEL}")]))
    }

    #[cfg(not(target_os = "macos"))]
    fn activate(&self) -> Result<Activation> {
        Ok(Activation::PendingLogin)
    }

    #[cfg(not(target_os = "macos"))]
    fn deactivate(&self) -> Result<()> {
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    fn is_running(&self) -> Option<bool> {
        None
    }
}

/// The per-user launchd domain (`gui/<uid>`) this agent belongs in. A GUI-session
/// domain is required: capture and input need access to the user's display.
///
/// The uid comes from `id -u` rather than `libc::getuid` because this crate
/// forbids `unsafe`, and the cost of one process spawn is irrelevant on the
/// install/status path.
#[cfg(target_os = "macos")]
fn gui_domain() -> Result<String> {
    let out = Command::new("id")
        .arg("-u")
        .output()
        .map_err(|e| AgentError::Config(format!("could not determine the current uid: {e}")))?;
    let uid = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if uid.is_empty() || !uid.chars().all(|c| c.is_ascii_digit()) {
        return Err(AgentError::Config("could not determine the current uid".into()));
    }
    Ok(format!("gui/{uid}"))
}

/// Run a command, reporting whether it exited successfully. Output is discarded:
/// callers turn failure into an actionable message of their own.
#[cfg(target_os = "macos")]
fn run_ok(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Per-user log directory, following each platform's convention.
fn default_log_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| AgentError::Config("no home directory".into()))?;
    #[cfg(target_os = "macos")]
    let dir = home.join("Library/Logs/DeskSync");
    #[cfg(not(target_os = "macos"))]
    let dir = {
        let base = dirs::state_dir()
            .or_else(dirs::data_local_dir)
            .unwrap_or_else(|| home.join(".local/state"));
        base.join("desksync")
    };
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn manager_in(dir: &std::path::Path) -> ServiceManager {
        ServiceManager::with_autostart(
            Autostart::with_dir("/usr/local/bin/desksync-agent", dir.join("agents")).with_logs(dir.join("logs")),
        )
    }

    #[test]
    fn status_reports_not_installed_before_install() {
        let dir = tempdir().unwrap();
        let status = manager_in(dir.path()).status();
        assert!(!status.installed);
    }

    #[test]
    fn install_writes_an_entry_referencing_the_binary_and_logs() {
        let dir = tempdir().unwrap();
        let mgr = manager_in(dir.path());

        // Activation shells out to the platform service manager, which we don't
        // exercise here; the entry and log directory are what must be right.
        let _ = mgr.install();

        let entry = std::fs::read_to_string(mgr.entry_path()).expect("entry written");
        assert!(entry.contains("desksync-agent"));
        assert!(dir.path().join("logs").is_dir());
        assert!(mgr.status().installed);
    }

    #[test]
    fn uninstall_removes_the_entry_and_is_idempotent() {
        let dir = tempdir().unwrap();
        let mgr = manager_in(dir.path());
        let _ = mgr.install();

        mgr.uninstall().unwrap();
        assert!(!mgr.status().installed);
        mgr.uninstall().unwrap();
    }

    #[test]
    fn restart_without_install_explains_what_to_do() {
        let dir = tempdir().unwrap();
        let err = manager_in(dir.path()).restart().unwrap_err();
        assert!(err.to_string().contains("service install"), "got: {err}");
    }

    #[test]
    fn reconcile_entry_writes_the_same_entry_as_install() {
        // The daemon reconciles the entry on every start; if it wrote a different
        // entry than `install` did, log redirection would silently disappear.
        let dir = tempdir().unwrap();
        let mgr = manager_in(dir.path());

        mgr.reconcile_entry(true).unwrap();
        let reconciled = std::fs::read_to_string(mgr.entry_path()).unwrap();
        let _ = mgr.install();
        let installed = std::fs::read_to_string(mgr.entry_path()).unwrap();
        assert_eq!(reconciled, installed);

        mgr.reconcile_entry(false).unwrap();
        assert!(!mgr.status().installed);
    }

    #[test]
    fn log_path_is_a_file_inside_the_log_dir() {
        let dir = tempdir().unwrap();
        let log = manager_in(dir.path()).log_path().unwrap();
        assert!(log.starts_with(dir.path().join("logs")));
        assert_eq!(log.file_name().unwrap(), "agent.log");
    }
}
