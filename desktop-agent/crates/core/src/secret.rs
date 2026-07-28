//! OS-native secure credential storage for the desktop agent.
//!
//! Authentication material (JWT access/refresh tokens and the server-assigned
//! device id) must not live in the world-readable `config.json`. This module
//! abstracts a small key/value secret store behind the [`SecretStore`] trait so
//! the runtime never cares *where* secrets live:
//!
//! * [`KeyringSecretStore`] (behind the `os-keychain` feature) uses the OS
//!   credential store — macOS Keychain, Windows Credential Manager, or the Linux
//!   Secret Service — via the `keyring` crate. This is the production backend on
//!   desktop builds.
//! * [`FileSecretStore`] persists to an owner-only (`0600`) JSON file under the
//!   config directory. It is the fallback for headless CI and for Linux hosts
//!   without a running Secret Service, and it is what the default (non-native)
//!   build compiles so unit tests need no system keychain.
//!
//! Use [`default_secret_store`] to get the right backend for the current build.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{AgentError, Result};
use crate::store::atomic_write;

/// Service/namespace used for entries in the OS keychain.
pub const SECRET_SERVICE: &str = "com.desksync.agent";

/// Key under which the [`TokenBundle`] is stored.
pub const TOKENS_KEY: &str = "tokens";

/// Authentication material persisted between agent runs.
///
/// Derives [`Zeroize`]/[`ZeroizeOnDrop`] so the secret bytes are wiped from
/// memory on drop, and a redacted [`std::fmt::Debug`] so tokens never leak into
/// logs.
#[derive(Clone, Default, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct TokenBundle {
    /// Short-lived JWT access token.
    pub access_token: String,
    /// Long-lived refresh token used to mint new access tokens.
    pub refresh_token: String,
    /// Stable device identifier issued by the backend at registration.
    pub device_id: String,
}

impl std::fmt::Debug for TokenBundle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenBundle")
            .field("access_token", &Redacted(&self.access_token))
            .field("refresh_token", &Redacted(&self.refresh_token))
            .field("device_id", &self.device_id)
            .finish()
    }
}

struct Redacted<'a>(&'a str);

impl std::fmt::Debug for Redacted<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0.is_empty() {
            write!(f, "\"\"")
        } else {
            write!(f, "\"<redacted {} chars>\"", self.0.len())
        }
    }
}

/// A minimal key/value store for secrets. Values are opaque strings (callers
/// serialize richer types themselves, e.g. [`TokenBundle`] as JSON).
pub trait SecretStore: Send + Sync {
    /// Store (or overwrite) `value` under `key`.
    fn set(&self, key: &str, value: &str) -> Result<()>;
    /// Fetch the value for `key`, or `None` if it is not present.
    fn get(&self, key: &str) -> Result<Option<String>>;
    /// Remove `key`. Deleting a missing key is not an error.
    fn delete(&self, key: &str) -> Result<()>;
}

/// Serialize and persist a [`TokenBundle`].
pub fn save_tokens(store: &dyn SecretStore, tokens: &TokenBundle) -> Result<()> {
    let json = serde_json::to_string(tokens)?;
    store.set(TOKENS_KEY, &json)
}

/// Load and deserialize the persisted [`TokenBundle`], if any.
pub fn load_tokens(store: &dyn SecretStore) -> Result<Option<TokenBundle>> {
    match store.get(TOKENS_KEY)? {
        Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
        None => Ok(None),
    }
}

/// Remove any persisted [`TokenBundle`] (used by `logout`).
pub fn clear_tokens(store: &dyn SecretStore) -> Result<()> {
    store.delete(TOKENS_KEY)
}

/// Return the appropriate secret store for the current build: the OS keychain
/// when compiled with `os-keychain`, otherwise the owner-only file fallback
/// rooted at `dir`.
pub fn default_secret_store(dir: &Path) -> Box<dyn SecretStore> {
    #[cfg(feature = "os-keychain")]
    {
        let _ = dir;
        Box::new(KeyringSecretStore::new(SECRET_SERVICE))
    }
    #[cfg(not(feature = "os-keychain"))]
    {
        Box::new(FileSecretStore::new(dir))
    }
}

/// Owner-only JSON-file secret store (fallback backend).
///
/// All keys live in a single `secrets.json` written with `0600` permissions on
/// Unix. The agent is single-instance, so read-modify-write needs no extra
/// locking beyond the process lock already held by the daemon.
#[derive(Debug, Clone)]
pub struct FileSecretStore {
    path: PathBuf,
}

impl FileSecretStore {
    /// Create a file store whose `secrets.json` lives under `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            path: dir.into().join("secrets.json"),
        }
    }

    fn read_map(&self) -> Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(raw) => Ok(serde_json::from_str(&raw).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(AgentError::Secret(format!("reading {}: {e}", self.path.display()))),
        }
    }

    fn write_map(&self, map: &BTreeMap<String, String>) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AgentError::Secret(format!("creating {}: {e}", parent.display())))?;
        }
        let json = serde_json::to_string_pretty(map)?;
        atomic_write(&self.path, json.as_bytes(), 0o600)
    }
}

impl SecretStore for FileSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        let mut map = self.read_map()?;
        map.insert(key.to_string(), value.to_string());
        self.write_map(&map)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.read_map()?.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<()> {
        let mut map = self.read_map()?;
        if map.remove(key).is_some() {
            self.write_map(&map)?;
        }
        Ok(())
    }
}

/// OS keychain secret store (production backend; requires the `os-keychain`
/// feature). Each key is a separate credential-store entry under
/// [`SECRET_SERVICE`].
#[cfg(feature = "os-keychain")]
#[derive(Debug, Clone)]
pub struct KeyringSecretStore {
    service: String,
}

#[cfg(feature = "os-keychain")]
impl KeyringSecretStore {
    /// Create a keychain store namespaced under `service`.
    pub fn new(service: impl Into<String>) -> Self {
        Self {
            service: service.into(),
        }
    }

    fn entry(&self, key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(&self.service, key).map_err(|e| AgentError::Secret(e.to_string()))
    }
}

#[cfg(feature = "os-keychain")]
impl SecretStore for KeyringSecretStore {
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.entry(key)?
            .set_password(value)
            .map_err(|e| AgentError::Secret(e.to_string()))
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match self.entry(key)?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AgentError::Secret(e.to_string())),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AgentError::Secret(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn file_store_set_get_delete_roundtrip() {
        let dir = tempdir().unwrap();
        let store = FileSecretStore::new(dir.path());

        assert_eq!(store.get("missing").unwrap(), None);
        store.set("k", "v1").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v1"));
        // Overwrite.
        store.set("k", "v2").unwrap();
        assert_eq!(store.get("k").unwrap().as_deref(), Some("v2"));
        // Delete, then it's gone; deleting again is a no-op (not an error).
        store.delete("k").unwrap();
        assert_eq!(store.get("k").unwrap(), None);
        store.delete("k").unwrap();
    }

    #[test]
    fn token_bundle_roundtrips_through_store() {
        let dir = tempdir().unwrap();
        let store = FileSecretStore::new(dir.path());

        assert!(load_tokens(&store).unwrap().is_none());
        let tokens = TokenBundle {
            access_token: "acc".into(),
            refresh_token: "ref".into(),
            device_id: "dev-1".into(),
        };
        save_tokens(&store, &tokens).unwrap();

        let loaded = load_tokens(&store).unwrap().expect("tokens present");
        assert_eq!(loaded.access_token, "acc");
        assert_eq!(loaded.refresh_token, "ref");
        assert_eq!(loaded.device_id, "dev-1");

        clear_tokens(&store).unwrap();
        assert!(load_tokens(&store).unwrap().is_none());
    }

    #[test]
    fn debug_redacts_tokens() {
        let tokens = TokenBundle {
            access_token: "supersecret".into(),
            refresh_token: "anothersecret".into(),
            device_id: "dev-1".into(),
        };
        let printed = format!("{tokens:?}");
        assert!(!printed.contains("supersecret"));
        assert!(!printed.contains("anothersecret"));
        // Non-secret device id is fine to show.
        assert!(printed.contains("dev-1"));
    }

    #[cfg(unix)]
    #[test]
    fn secrets_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let store = FileSecretStore::new(dir.path());
        store.set("k", "v").unwrap();

        let perms = std::fs::metadata(dir.path().join("secrets.json")).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }
}
