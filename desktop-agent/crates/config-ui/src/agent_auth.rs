//! Credential and device bootstrap for the agent.
//!
//! Turns whatever is on this machine — stored keychain credentials, or password
//! credentials in the environment — into a ready-to-use [`AuthSession`], and
//! makes sure this desktop is registered as a device so the phone can see it.
//!
//! Order of preference for credentials:
//! 1. **Stored tokens** from the OS keychain (written by `desksync-agent login`).
//!    This is the normal path: no credentials in the environment at all.
//! 2. **`DESKSYNC_EMAIL`/`DESKSYNC_PASSWORD`** — kept for CI and headless boxes,
//!    and also used as the refresh fallback when a stored refresh token dies.
//! 3. Nothing: the agent runs in a degraded, signed-out state and tells the user
//!    to run `login`.
//!
//! Device registration is idempotent on the backend (keyed by the device public
//! key), so it is safe to call whenever the local device id is missing.

use std::sync::{Arc, Mutex};

use anyhow::Context;
use desksync_backend::{
    detect_device_name, detect_platform, AuthSession, BackendApi, BackendClient, BackendError, Credentials,
    DeviceRegistration, TokenPair, TokenSink,
};
use desksync_core::identity::DeviceIdentity;
use desksync_core::{default_secret_store, load_tokens, save_tokens, AgentConfig, AgentStore, SecretStore, TokenBundle};

/// Placeholder written into a fresh config before the backend assigns an id.
const UNREGISTERED: &str = "unregistered";

/// Whether a device id refers to an actually-registered device.
fn is_registered(device_id: &str) -> bool {
    let id = device_id.trim();
    !id.is_empty() && id != UNREGISTERED
}

/// Persists rotated tokens into the OS credential store, keeping the device id
/// stored alongside them.
pub struct KeychainTokenSink {
    secrets: Arc<dyn SecretStore>,
    device_id: Mutex<String>,
}

impl KeychainTokenSink {
    /// Build a sink over a secret store, recording tokens against `device_id`.
    pub fn new(secrets: Arc<dyn SecretStore>, device_id: impl Into<String>) -> Self {
        Self {
            secrets,
            device_id: Mutex::new(device_id.into()),
        }
    }

    /// Update the device id recorded with future token writes (after the backend
    /// assigns one at registration).
    pub fn set_device_id(&self, device_id: &str) {
        *self.device_id.lock().expect("device id mutex poisoned") = device_id.to_string();
    }

    fn current_device_id(&self) -> String {
        self.device_id.lock().expect("device id mutex poisoned").clone()
    }
}

impl TokenSink for KeychainTokenSink {
    fn persist(&self, tokens: &TokenPair) -> desksync_backend::Result<()> {
        let bundle = TokenBundle {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone(),
            device_id: self.current_device_id(),
        };
        save_tokens(self.secrets.as_ref(), &bundle).map_err(|e| BackendError::Invalid(e.to_string()))
    }
}

/// A signed-in agent: an authenticated backend session plus the registered
/// device id everything else keys off.
pub struct AgentSession {
    /// Authenticated backend session with automatic token rotation. It holds the
    /// sink internally, so rotated tokens are persisted without callers helping.
    pub session: Arc<AuthSession>,
    /// The backend-assigned device id for this desktop.
    pub device_id: String,
}

/// Build an [`AgentSession`] from stored or environment credentials, registering
/// this desktop if it does not have a device id yet.
///
/// Returns `Ok(None)` when there are no credentials at all — the caller should
/// keep running in a signed-out state rather than failing, so the user can sign
/// in without restarting anything.
pub async fn bootstrap(
    store: &AgentStore,
    config: &AgentConfig,
    identity: &DeviceIdentity,
) -> anyhow::Result<Option<AgentSession>> {
    let secrets: Arc<dyn SecretStore> = Arc::from(default_secret_store(store.dir()));
    // A malformed/unreadable secret store must not stop the agent: treat it as
    // "not signed in" and let the user re-run `login`.
    let stored = load_tokens(secrets.as_ref()).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "could not read stored credentials; treating as signed out");
        None
    });
    let env_creds = Credentials::from_env().ok();

    // `config.json` is the single source of truth for device identity. The copy in
    // the credential bundle is only a diagnostic record: reading it back would
    // break the documented "set device_id to unregistered to re-register" escape
    // hatch, e.g. after repointing the agent at a different backend where the old
    // id does not exist. Re-registration is idempotent (keyed by the device public
    // key), so redoing it costs nothing and returns the same id.
    let device_id = config.device_id.clone();

    let api: Arc<dyn BackendApi> =
        Arc::new(BackendClient::new(&config.api_url).context("building backend client")?);
    let sink = Arc::new(KeychainTokenSink::new(Arc::clone(&secrets), device_id.clone()));

    let session = match stored {
        Some(bundle) if !bundle.refresh_token.is_empty() => {
            tracing::info!("using stored credentials from {}", secret_backend_label());
            Arc::new(AuthSession::new(
                api,
                TokenPair {
                    access_token: bundle.access_token.clone(),
                    refresh_token: bundle.refresh_token.clone(),
                    token_type: "Bearer".into(),
                    expires_in: 0,
                },
                Some(Arc::clone(&sink) as Arc<dyn TokenSink>),
                env_creds,
            ))
        }
        _ => match env_creds {
            Some(creds) => {
                tracing::info!("no stored credentials; authenticating with DESKSYNC_EMAIL/DESKSYNC_PASSWORD");
                Arc::new(
                    AuthSession::login(api, creds, Some(Arc::clone(&sink) as Arc<dyn TokenSink>))
                        .await
                        .context("login failed")?,
                )
            }
            None => return Ok(None),
        },
    };

    let device_id = ensure_registered(&session, &sink, store, config, identity, device_id).await?;
    Ok(Some(AgentSession { session, device_id }))
}

/// Register this desktop if it has no device id yet, persisting the assigned id
/// to both the config file and the stored credential bundle.
async fn ensure_registered(
    session: &Arc<AuthSession>,
    sink: &Arc<KeychainTokenSink>,
    store: &AgentStore,
    config: &AgentConfig,
    identity: &DeviceIdentity,
    current: String,
) -> anyhow::Result<String> {
    if is_registered(&current) {
        return Ok(current);
    }

    let device = session
        .register_device(&DeviceRegistration {
            kind: "desktop".into(),
            platform: detect_platform(),
            name: detect_device_name(),
            public_key: identity.public_base64(),
            fcm_token: None,
        })
        .await
        .context("registering this desktop as a device")?;
    tracing::info!(device_id = %device.id, name = %device.name, "desktop registered automatically");

    let mut updated = config.clone();
    updated.device_id = device.id.clone();
    if let Err(e) = store.save_config(&updated) {
        tracing::warn!(error = %e, "failed to persist the assigned device id");
    }

    // Re-store the credentials so the bundle carries the new device id too.
    sink.set_device_id(&device.id);
    let tokens = TokenPair {
        access_token: session.access_token().await,
        refresh_token: session.refresh_token().await,
        token_type: "Bearer".into(),
        expires_in: 0,
    };
    if let Err(e) = sink.persist(&tokens) {
        tracing::warn!(error = %e, "failed to persist credentials after registration");
    }

    Ok(device.id)
}

/// Human-readable label for where secrets are persisted in this build.
pub fn secret_backend_label() -> &'static str {
    if cfg!(feature = "native") {
        "the OS keychain"
    } else {
        "an owner-only file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use desksync_core::FileSecretStore;
    use tempfile::tempdir;

    #[test]
    fn registered_ids_are_recognized() {
        assert!(is_registered("f7c1e0f4-1234"));
        assert!(!is_registered(""));
        assert!(!is_registered("   "));
        // The placeholder must count as "needs registration", which is what makes
        // editing config.json a working way to force a re-register.
        assert!(!is_registered(UNREGISTERED));
    }

    #[test]
    fn sink_writes_tokens_with_the_current_device_id() {
        let dir = tempdir().unwrap();
        let secrets: Arc<dyn SecretStore> = Arc::new(FileSecretStore::new(dir.path()));
        let sink = KeychainTokenSink::new(Arc::clone(&secrets), UNREGISTERED);

        let tokens = TokenPair {
            access_token: "a1".into(),
            refresh_token: "r1".into(),
            token_type: "Bearer".into(),
            expires_in: 900,
        };
        sink.persist(&tokens).unwrap();

        let stored = load_tokens(secrets.as_ref()).unwrap().unwrap();
        assert_eq!(stored.access_token, "a1");
        assert_eq!(stored.device_id, UNREGISTERED);

        // After registration the id is recorded with subsequent writes.
        sink.set_device_id("device-9");
        sink.persist(&tokens).unwrap();
        let stored = load_tokens(secrets.as_ref()).unwrap().unwrap();
        assert_eq!(stored.device_id, "device-9");
        assert_eq!(stored.refresh_token, "r1");
    }
}
