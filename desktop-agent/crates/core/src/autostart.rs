//! Launch-at-login (autostart) management.
//!
//! The agent is a background service, so users expect it to start on login.
//! Each desktop platform has its own mechanism:
//! - **macOS**: a LaunchAgent plist in `~/Library/LaunchAgents`.
//! - **Linux**: an XDG `.desktop` entry in `~/.config/autostart`.
//! - **Windows**: a launcher script in the Startup folder.
//!
//! The entry *content* and *filename* are computed by pure functions (unit
//! tested on every platform), while [`Autostart::enable`]/[`Autostart::disable`]
//! perform the filesystem side effects. The target directory is injectable so
//! the side effects can be tested against a temporary directory.

use crate::error::{AgentError, Result};
use std::fs;
use std::path::{Path, PathBuf};

/// The reverse-DNS label / base name used for the autostart entry.
pub const LABEL: &str = "com.desksync.agent";

/// Manages this host's autostart entry for a given executable.
#[derive(Debug, Clone)]
pub struct Autostart {
    exec_path: PathBuf,
    dir: PathBuf,
    log_dir: Option<PathBuf>,
}

impl Autostart {
    /// Build an autostart manager for `exec_path`, writing entries into `dir`.
    pub fn with_dir(exec_path: impl Into<PathBuf>, dir: impl Into<PathBuf>) -> Self {
        Self {
            exec_path: exec_path.into(),
            dir: dir.into(),
            log_dir: None,
        }
    }

    /// Build an autostart manager for the current executable, using the
    /// platform-standard autostart directory.
    pub fn for_current_exe() -> Result<Self> {
        let exec = std::env::current_exe()?;
        let dir = default_dir()?;
        Ok(Self::with_dir(exec, dir))
    }

    /// Redirect the started process's output into `dir`.
    ///
    /// A service started at login has no terminal, so without this its logs are
    /// discarded and failures are invisible. Only the platforms whose autostart
    /// mechanism supports redirection (launchd) honour this.
    pub fn with_logs(mut self, dir: impl Into<PathBuf>) -> Self {
        self.log_dir = Some(dir.into());
        self
    }

    /// The full path to the autostart entry file.
    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(entry_file_name())
    }

    /// Where the service's stdout/stderr are written, if configured.
    pub fn log_path(&self) -> Option<PathBuf> {
        self.log_dir.as_ref().map(|d| d.join("agent.log"))
    }

    /// Whether the autostart entry currently exists.
    pub fn is_enabled(&self) -> bool {
        self.entry_path().exists()
    }

    /// The textual content of the autostart entry for this platform.
    pub fn entry_contents(&self) -> String {
        render_entry(&self.exec_path, self.log_path().as_deref())
    }

    /// Create the autostart entry (idempotent).
    pub fn enable(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        if let Some(log_dir) = &self.log_dir {
            // launchd will not create the directory for its log paths, and a
            // missing one makes the job fail to spawn.
            fs::create_dir_all(log_dir)?;
        }
        fs::write(self.entry_path(), self.entry_contents())?;
        Ok(())
    }

    /// Remove the autostart entry (idempotent).
    pub fn disable(&self) -> Result<()> {
        let path = self.entry_path();
        if path.exists() {
            fs::remove_file(path)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn entry_file_name() -> String {
    format!("{LABEL}.plist")
}
#[cfg(target_os = "linux")]
fn entry_file_name() -> String {
    "desksync-agent.desktop".to_string()
}
#[cfg(target_os = "windows")]
fn entry_file_name() -> String {
    "DeskSyncAgent.cmd".to_string()
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn entry_file_name() -> String {
    "desksync-agent.autostart".to_string()
}

#[cfg(target_os = "macos")]
fn render_entry(exec: &Path, log: Option<&Path>) -> String {
    // ProcessType=Background tells the scheduler this is a long-running daemon
    // rather than an interactive app, so it isn't throttled as aggressively.
    let logging = match log {
        Some(path) => format!(
            "    <key>StandardOutPath</key>\n    <string>{path}</string>\n\
             \x20   <key>StandardErrorPath</key>\n    <string>{path}</string>\n",
            path = path.display()
        ),
        None => String::new(),
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exec}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ProcessType</key>
    <string>Background</string>
{logging}</dict>
</plist>
"#,
        exec = exec.display()
    )
}

#[cfg(target_os = "linux")]
fn render_entry(exec: &Path, log: Option<&Path>) -> String {
    // XDG autostart has no log redirection, so wrap the command in a shell when
    // a log path is configured.
    let command = match log {
        Some(path) => format!("sh -c '\"{exec}\" >> \"{log}\" 2>&1'", exec = exec.display(), log = path.display()),
        None => exec.display().to_string(),
    };
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=DeskSync Agent\n\
         Exec={command}\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

#[cfg(target_os = "windows")]
fn render_entry(exec: &Path, log: Option<&Path>) -> String {
    match log {
        Some(path) => format!(
            "@echo off\r\nstart \"\" /b \"{exec}\" >> \"{log}\" 2>&1\r\n",
            exec = exec.display(),
            log = path.display()
        ),
        None => format!("@echo off\r\nstart \"\" \"{exec}\"\r\n", exec = exec.display()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn render_entry(exec: &Path, _log: Option<&Path>) -> String {
    format!("exec={}\n", exec.display())
}

#[cfg(target_os = "macos")]
fn default_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| AgentError::Config("no home directory".into()))?;
    Ok(home.join("Library/LaunchAgents"))
}
#[cfg(target_os = "linux")]
fn default_dir() -> Result<PathBuf> {
    let cfg = dirs::config_dir().ok_or_else(|| AgentError::Config("no config directory".into()))?;
    Ok(cfg.join("autostart"))
}
#[cfg(target_os = "windows")]
fn default_dir() -> Result<PathBuf> {
    let data = dirs::data_dir().ok_or_else(|| AgentError::Config("no data directory".into()))?;
    Ok(data.join("Microsoft/Windows/Start Menu/Programs/Startup"))
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn default_dir() -> Result<PathBuf> {
    Err(AgentError::Config("autostart is not supported on this platform".into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn entry_contents_reference_the_executable() {
        let auto = Autostart::with_dir("/opt/desksync/desksync-agent", "/tmp/does-not-matter");
        let contents = auto.entry_contents();
        assert!(contents.contains("/opt/desksync/desksync-agent"));
        assert!(!contents.is_empty());
    }

    #[test]
    fn enable_then_disable_roundtrips() {
        let dir = tempdir().unwrap();
        let auto = Autostart::with_dir("/usr/local/bin/desksync-agent", dir.path());

        assert!(!auto.is_enabled());
        auto.enable().unwrap();
        assert!(auto.is_enabled());

        let written = fs::read_to_string(auto.entry_path()).unwrap();
        assert!(written.contains("desksync-agent"));

        // enable is idempotent.
        auto.enable().unwrap();
        assert!(auto.is_enabled());

        auto.disable().unwrap();
        assert!(!auto.is_enabled());
        // disable is idempotent.
        auto.disable().unwrap();
    }

    #[test]
    fn entry_path_is_under_target_dir() {
        let auto = Autostart::with_dir("/bin/x", "/tmp/autostart-dir");
        assert!(auto.entry_path().starts_with("/tmp/autostart-dir"));
    }

    #[test]
    fn configured_logs_appear_in_the_entry() {
        let auto = Autostart::with_dir("/bin/desksync-agent", "/tmp/d").with_logs("/tmp/logs");
        assert_eq!(auto.log_path().unwrap(), Path::new("/tmp/logs/agent.log"));
        assert!(
            auto.entry_contents().contains("/tmp/logs/agent.log"),
            "a login-started service has no terminal, so the entry must redirect output: {}",
            auto.entry_contents()
        );
    }

    #[test]
    fn enable_creates_the_log_directory() {
        // launchd refuses to spawn a job whose log directory is missing.
        let dir = tempdir().unwrap();
        let logs = dir.path().join("logs");
        let auto = Autostart::with_dir("/bin/desksync-agent", dir.path().join("agents")).with_logs(&logs);

        auto.enable().unwrap();
        assert!(logs.is_dir());
    }
}
