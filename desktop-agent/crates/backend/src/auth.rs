//! An authenticated backend session that transparently rotates tokens.
//!
//! Every long-lived agent task (heartbeats, session polling) needs a valid access
//! token, and access tokens are deliberately short-lived. Rather than making each
//! call site handle "401 → refresh → retry → persist", that logic lives here once
//! and the rest of the agent calls domain methods ([`AuthSession::heartbeat`],
//! [`AuthSession::pending_sessions`], …) that just work.
//!
//! Token lifecycle:
//! 1. Try the request with the current access token.
//! 2. On `401`, rotate the refresh token for a new pair and retry **once**.
//! 3. If rotation fails, adopt a pair another process has stored since, then fall
//!    back to password credentials when they were supplied — that path exists for
//!    CI and headless boxes; interactive users re-run `desksync-agent login`.
//! 4. Whenever the pair changes, hand it to the [`TokenSink`] so a restart does
//!    not require re-authentication.
//!
//! **A refresh token must never be presented twice.** The backend rotates refresh
//! tokens and treats a repeat as evidence of theft, revoking every token the user
//! has — so a careless retry does not merely fail, it logs the account out
//! everywhere. Two things here exist solely to guarantee single use:
//!
//! - Rotation is **single-flight**. Long-lived tasks (heartbeat, session polling)
//!   hit `401` at the same moment, because they share one access token and it
//!   expires for all of them at once. Without serialization they each refresh with
//!   the same token, and the second one trips reuse detection.
//! - A token the backend has already refused is remembered and never sent again,
//!   so a retry loop cannot turn one expiry into a stream of reuse reports.

use std::collections::HashSet;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};

use crate::client::BackendApi;
use crate::error::{BackendError, Result};
use crate::models::{Device, DeviceRegistration, PairingChallenge, PendingSession, TokenPair};

/// Password credentials for a DeskSync account.
///
/// Interactive users sign in through the browser ([`crate::oauth`]) and never
/// supply these; they exist for CI and headless hosts, and as the fallback when a
/// stored refresh token is rejected.
#[derive(Debug, Clone)]
pub struct Credentials {
    /// Account email.
    pub email: String,
    /// Account password.
    pub password: String,
}

impl Credentials {
    /// Load credentials from `DESKSYNC_EMAIL` / `DESKSYNC_PASSWORD`.
    pub fn from_env() -> Result<Self> {
        let email = std::env::var("DESKSYNC_EMAIL").ok().filter(|s| !s.trim().is_empty());
        let password = std::env::var("DESKSYNC_PASSWORD").ok().filter(|s| !s.trim().is_empty());
        match (email, password) {
            (Some(email), Some(password)) => Ok(Self { email, password }),
            _ => Err(BackendError::Invalid(
                "set DESKSYNC_EMAIL and DESKSYNC_PASSWORD, or run `desksync-agent login`".into(),
            )),
        }
    }
}

/// Receives token pairs whenever they change, so they survive a restart.
///
/// The agent implements this over the OS keychain; tests use a capturing fake.
pub trait TokenSink: Send + Sync {
    /// Persist the latest token pair. Errors are logged by the caller and never
    /// abort the request that triggered the rotation.
    fn persist(&self, tokens: &TokenPair) -> Result<()>;

    /// Read back the stored pair, if the store can be read.
    ///
    /// This matters because the store is **shared between processes**: the
    /// background service, `setup`, and any CLI command all use the same keychain
    /// entry, and refreshing rotates the token server-side. When another process
    /// rotates, this process's in-memory refresh token becomes invalid even though
    /// perfectly good credentials are sitting in the store — see
    /// [`AuthSession::reauthenticate`].
    ///
    /// Defaults to `None` so implementations that only persist keep working; they
    /// simply do not get cross-process recovery.
    fn load(&self) -> Result<Option<TokenPair>> {
        Ok(None)
    }
}

/// An authenticated view of the backend for one account/device.
pub struct AuthSession {
    api: Arc<dyn BackendApi>,
    tokens: RwLock<TokenPair>,
    sink: Option<Arc<dyn TokenSink>>,
    fallback: Option<Credentials>,
    /// Held for the duration of a rotation so only one is ever in flight.
    rotating: Mutex<()>,
    /// Incremented every time `tokens` is replaced. A caller that saw generation
    /// `n` and finds `n + 1` after waiting knows someone else already rotated and
    /// it should simply retry, rather than rotating again with a spent token.
    generation: AtomicU64,
    /// Refresh tokens the backend has already refused. Re-sending one is what
    /// reuse detection punishes, so they are never presented again.
    rejected: RwLock<HashSet<String>>,
}

impl AuthSession {
    /// Build a session around an existing token pair (e.g. loaded from the OS
    /// keychain).
    pub fn new(
        api: Arc<dyn BackendApi>,
        tokens: TokenPair,
        sink: Option<Arc<dyn TokenSink>>,
        fallback: Option<Credentials>,
    ) -> Self {
        Self {
            api,
            tokens: RwLock::new(tokens),
            sink,
            fallback,
            rotating: Mutex::new(()),
            generation: AtomicU64::new(0),
            rejected: RwLock::new(HashSet::new()),
        }
    }

    /// Authenticate with password credentials and build a session from the
    /// resulting pair, persisting it immediately.
    pub async fn login(api: Arc<dyn BackendApi>, creds: Credentials, sink: Option<Arc<dyn TokenSink>>) -> Result<Self> {
        let tokens = api.login(&creds.email, &creds.password).await?;
        let session = Self::new(api, tokens, sink, Some(creds));
        session.persist().await;
        Ok(session)
    }

    /// The current access token. Prefer the domain methods, which also handle
    /// expiry; this exists for callers that must pass a bearer token onward.
    pub async fn access_token(&self) -> String {
        self.tokens.read().await.access_token.clone()
    }

    /// The current refresh token, for persisting alongside other agent state.
    pub async fn refresh_token(&self) -> String {
        self.tokens.read().await.refresh_token.clone()
    }

    /// Report presence for a device.
    pub async fn heartbeat(&self, device_id: &str) -> Result<()> {
        let api = Arc::clone(&self.api);
        let id = device_id.to_string();
        self.retrying(move |token| {
            let api = Arc::clone(&api);
            let id = id.clone();
            async move { api.heartbeat(&token, &id).await }
        })
        .await
    }

    /// List sessions this device should answer.
    pub async fn pending_sessions(&self, device_id: &str) -> Result<Vec<PendingSession>> {
        let api = Arc::clone(&self.api);
        let id = device_id.to_string();
        self.retrying(move |token| {
            let api = Arc::clone(&api);
            let id = id.clone();
            async move { api.pending_sessions(&token, &id).await }
        })
        .await
    }

    /// Register (or idempotently re-register) a device.
    pub async fn register_device(&self, reg: &DeviceRegistration) -> Result<Device> {
        let api = Arc::clone(&self.api);
        let reg = reg.clone();
        self.retrying(move |token| {
            let api = Arc::clone(&api);
            let reg = reg.clone();
            async move { api.register_device(&token, &reg).await }
        })
        .await
    }

    /// Initiate a pairing for one of this account's desktop devices.
    pub async fn initiate_pairing(&self, desktop_device_id: &str) -> Result<PairingChallenge> {
        let api = Arc::clone(&self.api);
        let id = desktop_device_id.to_string();
        self.retrying(move |token| {
            let api = Arc::clone(&api);
            let id = id.clone();
            async move { api.initiate_pairing(&token, &id).await }
        })
        .await
    }

    /// Run an authorized operation, rotating tokens and retrying once if the
    /// access token has expired.
    async fn retrying<T, F, Fut>(&self, op: F) -> Result<T>
    where
        F: Fn(String) -> Fut,
        Fut: Future<Output = Result<T>>,
    {
        // Capture which token generation this attempt used, so that if it fails we
        // can tell "my token is stale" from "someone already replaced it".
        let generation = self.generation.load(Ordering::Acquire);
        let token = self.access_token().await;
        match op(token).await {
            Err(e) if is_unauthorized(&e) => {
                self.rotate(generation).await?;
                op(self.access_token().await).await
            }
            other => other,
        }
    }

    /// Replace the token pair, unless another caller already did it first.
    ///
    /// `seen_generation` is the generation the failed request used. Concurrent
    /// tasks all fail at the same moment with the same expired access token; the
    /// first one through the lock rotates, and the rest observe a newer generation
    /// and return so they retry with the token that was just minted. Skipping that
    /// check would mean presenting the same refresh token several times, which the
    /// backend reads as theft and answers by revoking every token for the account.
    async fn rotate(&self, seen_generation: u64) -> Result<()> {
        let _guard = self.rotating.lock().await;
        if self.generation.load(Ordering::Acquire) != seen_generation {
            return Ok(());
        }
        self.reauthenticate().await
    }

    /// Obtain a fresh token pair, in order of least disruption: rotate the refresh
    /// token; adopt a pair another process has stored since; or fall back to
    /// password credentials when one was supplied.
    pub async fn reauthenticate(&self) -> Result<()> {
        let refresh = self.refresh_token().await;
        let rotated = match self.try_refresh(&refresh).await {
            Some(pair) => Some(pair),
            None => match self.rotated_elsewhere(&refresh).await {
                Some(pair) => Some(pair),
                None => match &self.fallback {
                    Some(creds) => Some(self.api.login(&creds.email, &creds.password).await?),
                    None => None,
                },
            },
        };
        let Some(pair) = rotated else {
            return Err(BackendError::Invalid(
                "session expired; run `desksync-agent login` to sign in again".into(),
            ));
        };

        *self.tokens.write().await = pair;
        self.generation.fetch_add(1, Ordering::Release);
        self.persist().await;
        tracing::info!("rotated backend credentials");
        Ok(())
    }

    /// Present a refresh token once, remembering it if the backend refuses it.
    ///
    /// A token that has already been refused is not sent again: the backend rotates
    /// refresh tokens and reads a repeat as theft, revoking every token for the
    /// account. A retry loop that keeps re-presenting an expired token would
    /// therefore escalate a recoverable expiry into a full lockout.
    async fn try_refresh(&self, refresh: &str) -> Option<TokenPair> {
        if self.rejected.read().await.contains(refresh) {
            tracing::debug!("skipping a refresh token the backend already refused");
            return None;
        }
        match self.api.refresh(refresh).await {
            Ok(pair) => Some(pair),
            Err(e) => {
                tracing::debug!(error = %e, "token refresh failed; checking the shared credential store");
                self.rejected.write().await.insert(refresh.to_string());
                None
            }
        }
    }

    /// Recover a pair that another process rotated into the shared store.
    ///
    /// Refreshing invalidates the old refresh token server-side, so any other
    /// DeskSync process that refreshes — `setup`, `login`, a second agent — leaves
    /// this one holding a token the backend will now reject. Without this, a
    /// long-running service reports "session expired" forever while valid
    /// credentials sit in the keychain, and the only cure is a restart.
    ///
    /// Returns `None` when there is no store, it cannot be read, or it holds the
    /// same (already-rejected) refresh token — in which case the session really is
    /// expired and the caller should say so.
    async fn rotated_elsewhere(&self, tried: &str) -> Option<TokenPair> {
        let sink = Arc::clone(self.sink.as_ref()?);

        // Reading the OS credential store is a blocking call that can take seconds
        // (macOS may raise a keychain access dialog), so keep it off the runtime.
        let stored = tokio::task::spawn_blocking(move || sink.load()).await;
        let stored = match stored {
            Ok(Ok(Some(pair))) => pair,
            Ok(Ok(None)) => return None,
            Ok(Err(e)) => {
                tracing::debug!(error = %e, "could not read the credential store during reauth");
                return None;
            }
            Err(e) => {
                tracing::debug!(error = %e, "credential store read task failed during reauth");
                return None;
            }
        };

        if stored.refresh_token == tried {
            return None;
        }

        // The stored pair's access token may itself be stale, so rotate it rather
        // than adopting it blind: that both validates it and leaves us owning a
        // token the backend has just issued to us.
        match self.try_refresh(&stored.refresh_token).await {
            Some(pair) => {
                tracing::info!("adopted credentials rotated by another DeskSync process");
                Some(pair)
            }
            None => {
                tracing::debug!("stored credentials were rejected too");
                None
            }
        }
    }

    /// Hand the current pair to the sink. Persistence failures are logged, not
    /// propagated: the in-memory session is still usable.
    async fn persist(&self) {
        let Some(sink) = &self.sink else { return };
        let tokens = self.tokens.read().await;
        if let Err(e) = sink.persist(&tokens) {
            tracing::warn!(error = %e, "failed to persist rotated credentials");
        }
    }
}

/// Whether an error means "the access token is no longer accepted".
fn is_unauthorized(e: &BackendError) -> bool {
    matches!(e, BackendError::Api { status: 401, .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;

    fn pair(n: u32) -> TokenPair {
        TokenPair {
            access_token: format!("access-{n}"),
            refresh_token: format!("refresh-{n}"),
            token_type: "Bearer".into(),
            expires_in: 900,
        }
    }

    fn unauthorized() -> BackendError {
        BackendError::Api {
            status: 401,
            code: "unauthorized".into(),
            message: "expired".into(),
        }
    }

    /// A backend that only accepts `accepted_token`, counting calls. Refresh
    /// succeeds unless `refresh_fails` is set; login always succeeds.
    struct FakeApi {
        accepted_token: String,
        heartbeats: AtomicUsize,
        refreshes: AtomicUsize,
        logins: AtomicUsize,
        refresh_fails: bool,
        /// When set, only this refresh token is honoured — which is how a real
        /// backend behaves once another process has rotated the old one.
        accepted_refresh: Option<String>,
    }

    impl FakeApi {
        fn new(accepted: &str) -> Self {
            Self {
                accepted_token: accepted.into(),
                heartbeats: AtomicUsize::new(0),
                refreshes: AtomicUsize::new(0),
                logins: AtomicUsize::new(0),
                refresh_fails: false,
                accepted_refresh: None,
            }
        }
    }

    #[async_trait]
    impl BackendApi for FakeApi {
        async fn login(&self, _email: &str, _password: &str) -> Result<TokenPair> {
            self.logins.fetch_add(1, Ordering::SeqCst);
            Ok(pair(3))
        }

        async fn refresh(&self, refresh_token: &str) -> Result<TokenPair> {
            // Yield so a rotation is genuinely in flight while other tasks reach
            // their own 401. Without this the fake completes synchronously and the
            // concurrency tests would serialize by accident, passing whether or not
            // rotation is single-flight.
            tokio::task::yield_now().await;
            self.refreshes.fetch_add(1, Ordering::SeqCst);
            if self.refresh_fails {
                return Err(unauthorized());
            }
            if let Some(accepted) = &self.accepted_refresh {
                if refresh_token != accepted {
                    return Err(unauthorized());
                }
            }
            Ok(pair(2))
        }

        async fn register_device(&self, access_token: &str, reg: &DeviceRegistration) -> Result<Device> {
            if access_token != self.accepted_token {
                return Err(unauthorized());
            }
            Ok(Device {
                id: "device-1".into(),
                kind: reg.kind.clone(),
                platform: reg.platform.clone(),
                name: reg.name.clone(),
                status: "offline".into(),
            })
        }

        async fn initiate_pairing(&self, _access_token: &str, _id: &str) -> Result<PairingChallenge> {
            unreachable!("not exercised")
        }

        async fn heartbeat(&self, access_token: &str, _device_id: &str) -> Result<()> {
            tokio::task::yield_now().await;
            self.heartbeats.fetch_add(1, Ordering::SeqCst);
            if access_token != self.accepted_token {
                return Err(unauthorized());
            }
            Ok(())
        }

        async fn pending_sessions(&self, access_token: &str, _device_id: &str) -> Result<Vec<PendingSession>> {
            if access_token != self.accepted_token {
                return Err(unauthorized());
            }
            Ok(vec![])
        }
    }

    #[derive(Default)]
    struct CapturingSink {
        persisted: Mutex<Vec<String>>,
        /// What a read of the shared store returns, standing in for a pair another
        /// process wrote there.
        stored: Mutex<Option<TokenPair>>,
    }

    impl CapturingSink {
        fn holding(tokens: TokenPair) -> Self {
            Self {
                persisted: Mutex::new(Vec::new()),
                stored: Mutex::new(Some(tokens)),
            }
        }
    }

    impl TokenSink for CapturingSink {
        fn persist(&self, tokens: &TokenPair) -> Result<()> {
            self.persisted.lock().unwrap().push(tokens.access_token.clone());
            Ok(())
        }

        fn load(&self) -> Result<Option<TokenPair>> {
            Ok(self.stored.lock().unwrap().clone())
        }
    }

    #[tokio::test]
    async fn valid_token_passes_through_without_refresh() {
        let api = Arc::new(FakeApi::new("access-1"));
        let session = AuthSession::new(Arc::clone(&api) as Arc<dyn BackendApi>, pair(1), None, None);

        session.heartbeat("dev").await.unwrap();
        assert_eq!(api.refreshes.load(Ordering::SeqCst), 0, "no rotation needed");
        assert_eq!(api.heartbeats.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn expired_token_is_rotated_and_the_call_retried() {
        // The backend only accepts the *rotated* token, so the first attempt 401s.
        let api = Arc::new(FakeApi::new("access-2"));
        let sink = Arc::new(CapturingSink::default());
        let session = AuthSession::new(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            pair(1),
            Some(Arc::clone(&sink) as Arc<dyn TokenSink>),
            None,
        );

        session.heartbeat("dev").await.expect("retry should succeed");

        assert_eq!(api.refreshes.load(Ordering::SeqCst), 1, "rotated once");
        assert_eq!(api.heartbeats.load(Ordering::SeqCst), 2, "original + retry");
        assert_eq!(session.access_token().await, "access-2");
        // The rotated pair was handed to the sink so a restart reuses it.
        assert_eq!(sink.persisted.lock().unwrap().as_slice(), ["access-2"]);
    }

    #[tokio::test]
    async fn concurrent_expiries_cause_exactly_one_refresh() {
        // The bug this pins: heartbeat and session polling share one access token,
        // so they 401 together. Refreshing twice with the same token makes the
        // backend treat it as theft ("refresh token reuse detected") and revoke
        // every token for the account — turning a routine expiry into a lockout
        // that only a fresh sign-in clears.
        let api = Arc::new(FakeApi::new("access-2"));
        let session = Arc::new(AuthSession::new(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            pair(1),
            None,
            None,
        ));

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let session = Arc::clone(&session);
            tasks.push(tokio::spawn(async move { session.heartbeat("dev").await }));
        }
        for task in tasks {
            task.await.unwrap().expect("every caller should end up authenticated");
        }

        assert_eq!(
            api.refreshes.load(Ordering::SeqCst),
            1,
            "the refresh token must be presented exactly once"
        );
    }

    #[tokio::test]
    async fn a_refused_refresh_token_is_never_presented_again() {
        // The heartbeat loop retries forever. Re-sending a token the backend has
        // already refused would report reuse on every tick.
        let mut fake = FakeApi::new("access-9"); // nothing we hold is accepted
        fake.refresh_fails = true;
        let api = Arc::new(fake);
        let session = AuthSession::new(Arc::clone(&api) as Arc<dyn BackendApi>, pair(1), None, None);

        for _ in 0..5 {
            let _ = session.heartbeat("dev").await;
        }

        assert_eq!(
            api.refreshes.load(Ordering::SeqCst),
            1,
            "one rejection is enough; further attempts must not re-send it"
        );
    }

    #[tokio::test]
    async fn credentials_rotated_by_another_process_are_adopted() {
        // The real failure this reproduces: a background service holds refresh-1
        // in memory, another DeskSync process (setup, login) refreshes and thereby
        // invalidates it server-side, and the service is left holding a token the
        // backend now rejects while a good pair sits in the shared keychain.
        let mut fake = FakeApi::new("access-2");
        fake.accepted_refresh = Some("refresh-9".into());
        let api = Arc::new(fake);
        let sink = Arc::new(CapturingSink::holding(pair(9)));

        let session = AuthSession::new(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            pair(1),
            Some(Arc::clone(&sink) as Arc<dyn TokenSink>),
            None, // no password fallback: the store is the only way back
        );

        session
            .heartbeat("dev")
            .await
            .expect("should recover from the stored pair instead of expiring");

        assert_eq!(session.access_token().await, "access-2");
        assert_eq!(
            api.refreshes.load(Ordering::SeqCst),
            2,
            "the stale token, then the stored one"
        );
        assert_eq!(api.logins.load(Ordering::SeqCst), 0, "no password login needed");
    }

    #[tokio::test]
    async fn a_store_holding_the_same_rejected_token_reports_expiry() {
        // Nothing to recover: the stored pair is the one already refused, so the
        // session really is expired and must say so rather than retry forever.
        let mut fake = FakeApi::new("access-2");
        fake.refresh_fails = true;
        let api = Arc::new(fake);
        let sink = Arc::new(CapturingSink::holding(pair(1)));

        let session = AuthSession::new(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            pair(1),
            Some(Arc::clone(&sink) as Arc<dyn TokenSink>),
            None,
        );

        let err = session.heartbeat("dev").await.expect_err("must not loop");
        assert!(err.to_string().contains("session expired"), "got: {err}");
        assert_eq!(
            api.refreshes.load(Ordering::SeqCst),
            1,
            "the identical stored token is not retried"
        );
    }

    #[tokio::test]
    async fn falls_back_to_password_when_refresh_is_rejected() {
        let mut fake = FakeApi::new("access-3"); // only the login result is accepted
        fake.refresh_fails = true;
        let api = Arc::new(fake);
        let session = AuthSession::new(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            pair(1),
            None,
            Some(Credentials {
                email: "a@example.com".into(),
                password: "pw".into(),
            }),
        );

        session
            .heartbeat("dev")
            .await
            .expect("password fallback should recover");
        assert_eq!(api.logins.load(Ordering::SeqCst), 1);
        assert_eq!(session.access_token().await, "access-3");
    }

    #[tokio::test]
    async fn without_fallback_a_dead_refresh_token_is_a_clear_error() {
        let mut fake = FakeApi::new("never-accepted");
        fake.refresh_fails = true;
        let api = Arc::new(fake);
        let session = AuthSession::new(Arc::clone(&api) as Arc<dyn BackendApi>, pair(1), None, None);

        let err = session.heartbeat("dev").await.unwrap_err();
        assert!(
            err.to_string().contains("login"),
            "should tell the user what to do: {err}"
        );
        assert_eq!(api.logins.load(Ordering::SeqCst), 0, "no credentials to fall back to");
    }

    #[tokio::test]
    async fn retry_happens_at_most_once() {
        // Nothing is ever accepted, so the retry also 401s and the error surfaces
        // instead of looping forever.
        let api = Arc::new(FakeApi::new("never-accepted"));
        let session = AuthSession::new(Arc::clone(&api) as Arc<dyn BackendApi>, pair(1), None, None);

        assert!(session.heartbeat("dev").await.is_err());
        assert_eq!(api.heartbeats.load(Ordering::SeqCst), 2, "one retry, not a loop");
        assert_eq!(api.refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn login_persists_the_initial_pair() {
        let api = Arc::new(FakeApi::new("access-3"));
        let sink = Arc::new(CapturingSink::default());
        let session = AuthSession::login(
            Arc::clone(&api) as Arc<dyn BackendApi>,
            Credentials {
                email: "a@example.com".into(),
                password: "pw".into(),
            },
            Some(Arc::clone(&sink) as Arc<dyn TokenSink>),
        )
        .await
        .unwrap();

        assert_eq!(session.access_token().await, "access-3");
        assert_eq!(sink.persisted.lock().unwrap().as_slice(), ["access-3"]);
    }

    #[tokio::test]
    async fn register_device_is_authorized_and_rotates_when_needed() {
        let api = Arc::new(FakeApi::new("access-2"));
        let session = AuthSession::new(Arc::clone(&api) as Arc<dyn BackendApi>, pair(1), None, None);

        let device = session
            .register_device(&DeviceRegistration {
                kind: "desktop".into(),
                platform: "macos".into(),
                name: "Test".into(),
                public_key: "cHVibGlj".into(),
                fcm_token: None,
            })
            .await
            .unwrap();

        assert_eq!(device.id, "device-1");
        assert_eq!(api.refreshes.load(Ordering::SeqCst), 1);
    }
}
