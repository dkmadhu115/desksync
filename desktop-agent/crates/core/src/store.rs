//! On-disk persistence for the agent: its configuration and its private
//! identity key.
//!
//! Layout (under the platform config directory, e.g.
//! `~/Library/Application Support/desksync` on macOS,
//! `~/.config/desksync` on Linux, `%APPDATA%\desksync` on Windows):
//!
//! ```text
//! desksync/
//!   config.json     # AgentConfig (world-readable is acceptable)
//!   identity.key    # hex X25519 secret key (chmod 0600 on Unix)
//! ```
//!
//! The identity key is written with owner-only permissions. In a hardening
//! pass the key can be moved into the OS keychain (macOS Keychain / Windows
//! Credential Manager / Secret Service); the [`AgentStore`] API stays the same.

use crate::config::AgentConfig;
use crate::error::{AgentError, Result};
use crate::identity::DeviceIdentity;
use std::fs;
use std::path::{Path, PathBuf};

const CONFIG_FILE: &str = "config.json";
const IDENTITY_FILE: &str = "identity.key";

/// Environment variable that relocates the whole agent state directory.
pub const CONFIG_DIR_ENV: &str = "DESKSYNC_CONFIG_DIR";

/// Resolve the state directory from an optional override.
///
/// Split out from [`AgentStore::platform_default`] so it is testable without
/// mutating process-wide environment state.
fn resolve_dir(override_dir: Option<std::ffi::OsString>) -> Result<PathBuf> {
    if let Some(dir) = override_dir.filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let base = dirs::config_dir().ok_or_else(|| AgentError::Config("no OS config directory available".into()))?;
    Ok(base.join("desksync"))
}

/// Filesystem-backed store rooted at a per-user config directory.
#[derive(Debug, Clone)]
pub struct AgentStore {
    dir: PathBuf,
}

impl AgentStore {
    /// Create a store rooted at an explicit directory (used in tests and for
    /// custom deployments).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Create a store rooted at the OS-appropriate per-user config directory,
    /// or at `DESKSYNC_CONFIG_DIR` when that is set.
    ///
    /// The override exists so a second, fully isolated instance can be run — its
    /// own config, identity, instance lock, and IPC socket — without touching the
    /// real one. Useful for testing a build before installing it as the service.
    pub fn platform_default() -> Result<Self> {
        Ok(Self::at(resolve_dir(std::env::var_os(CONFIG_DIR_ENV))?))
    }

    /// The directory backing this store.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        Ok(())
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join(CONFIG_FILE)
    }

    fn identity_path(&self) -> PathBuf {
        self.dir.join(IDENTITY_FILE)
    }

    /// Whether a persisted configuration exists.
    pub fn config_exists(&self) -> bool {
        self.config_path().exists()
    }

    /// Load the persisted configuration, returning an error if none exists or
    /// it cannot be parsed.
    pub fn load_config(&self) -> Result<AgentConfig> {
        let raw = fs::read_to_string(self.config_path())?;
        let cfg: AgentConfig = serde_json::from_str(&raw)?;
        Ok(cfg)
    }

    /// Persist the configuration atomically (write to a temp file, then rename).
    pub fn save_config(&self, config: &AgentConfig) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(config)?;
        atomic_write(&self.config_path(), json.as_bytes(), 0o644)
    }

    /// Load the device identity, or generate and persist a new one on first
    /// run. This is the single point where the private key touches disk.
    pub fn load_or_create_identity(&self) -> Result<DeviceIdentity> {
        let path = self.identity_path();
        if path.exists() {
            let hex_str = fs::read_to_string(&path)?;
            return DeviceIdentity::from_secret_hex(&hex_str);
        }
        let identity = DeviceIdentity::generate()?;
        self.ensure_dir()?;
        // Owner read/write only — the private key must never be group/world
        // readable.
        atomic_write(&path, identity.secret_hex().as_bytes(), 0o600)?;
        tracing::info!(fingerprint = %identity.fingerprint(), "generated new device identity");
        Ok(identity)
    }
}

/// Write `bytes` to `path` atomically: write to a sibling temp file, set its
/// permissions, then rename over the destination. `mode` is applied on Unix and
/// ignored elsewhere.
pub(crate) fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    set_mode(&tmp, mode)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn an_override_relocates_the_whole_state_directory() {
        let dir = resolve_dir(Some("/tmp/desksync-alt".into())).unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/desksync-alt"));
    }

    #[test]
    fn an_empty_override_falls_back_to_the_platform_directory() {
        // An exported-but-empty variable is a common shell accident; treating it
        // as "use the default" avoids writing state into the filesystem root.
        let dir = resolve_dir(Some("".into())).unwrap();
        assert!(dir.ends_with("desksync"));
        let default = resolve_dir(None).unwrap();
        assert_eq!(dir, default);
    }

    #[test]
    fn config_roundtrips_on_disk() {
        let dir = tempdir().unwrap();
        let store = AgentStore::at(dir.path());
        assert!(!store.config_exists());

        let cfg = AgentConfig {
            device_id: "dev-xyz".into(),
            backend_url: "wss://api.example.com/signaling".into(),
            target_fps: 45,
            ..Default::default()
        };
        store.save_config(&cfg).unwrap();

        assert!(store.config_exists());
        let loaded = store.load_config().unwrap();
        assert_eq!(loaded.device_id, "dev-xyz");
        assert_eq!(loaded.target_fps, 45);
    }

    #[test]
    fn identity_is_created_once_and_reused() {
        let dir = tempdir().unwrap();
        let store = AgentStore::at(dir.path());

        let first = store.load_or_create_identity().unwrap();
        let second = store.load_or_create_identity().unwrap();
        // Stable across reloads: same key is persisted and read back.
        assert_eq!(first.secret_hex(), second.secret_hex());
        assert_eq!(first.public_hex(), second.public_hex());
    }

    #[cfg(unix)]
    #[test]
    fn identity_key_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let store = AgentStore::at(dir.path());
        store.load_or_create_identity().unwrap();

        let perms = fs::metadata(dir.path().join(IDENTITY_FILE)).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn missing_config_is_an_error() {
        let dir = tempdir().unwrap();
        let store = AgentStore::at(dir.path());
        assert!(store.load_config().is_err());
    }
}
