//! Agent session runtime (native builds only).
//!
//! This is the piece that makes "connect from the phone" actually work. It:
//!
//! 1. Polls the backend for **pending sessions** targeting this desktop device,
//!    each carrying an agent signaling ticket + ICE configuration.
//! 2. For each new session, connects to the signaling service as the `agent`,
//!    builds a WebRTC [`AgentPeer`] (the answerer), and drives the
//!    offer/answer/ICE handshake via the pure [`NegotiationState`] machine.
//! 3. Streams JPEG-encoded screen frames over the `frames` data channel and
//!    dispatches inbound `input`/`control` frames to the injector and dev-tools.
//!
//! Authentication rides on the shared [`AuthSession`], which owns token rotation
//! and persistence, so this module never sees credentials.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use desksync_backend::{AuthSession, PendingSession};
use desksync_capture::CaptureLoop;
use desksync_devtools::DevToolsService;
use desksync_input::InputRouter;
use desksync_media::{AgentPeer, IceServer, JpegScreenEncoder, PeerConfig, PeerEvent};
use desksync_transport::{
    NegotiationAction, NegotiationRole, NegotiationState, SignalEnvelope, SignalPayload, SignalingTransport,
    WebSocketSignaling,
};
use tokio::sync::Mutex;

use crate::service_state::ServiceState;

/// How often to poll the backend for pending sessions.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Target frame interval for the stream (~15 fps) to bound CPU/bandwidth.
const FRAME_INTERVAL: Duration = Duration::from_millis(66);
/// Drop new frames when the data channel has more than this buffered (bytes).
const MAX_BUFFERED: usize = 2 * 1024 * 1024;

/// Everything the runtime needs to serve sessions.
pub struct SessionManager {
    auth: Arc<AuthSession>,
    device_id: String,
    capture: Arc<CaptureLoop>,
    input: Arc<InputRouter>,
    devtools: Arc<DevToolsService>,
    state: Arc<ServiceState>,
}

impl SessionManager {
    /// Build a session manager.
    pub fn new(
        auth: Arc<AuthSession>,
        device_id: String,
        capture: Arc<CaptureLoop>,
        input: Arc<InputRouter>,
        devtools: Arc<DevToolsService>,
        state: Arc<ServiceState>,
    ) -> Self {
        Self {
            auth,
            device_id,
            capture,
            input,
            devtools,
            state,
        }
    }

    /// Run the discovery loop forever. Intended to be spawned as a task.
    pub async fn run(self: Arc<Self>) {
        tracing::info!(device_id = %self.device_id, "session runtime ready; watching for incoming sessions");

        let active: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));

        loop {
            tokio::time::sleep(POLL_INTERVAL).await;

            let pending = match self.auth.pending_sessions(&self.device_id).await {
                Ok(p) => p,
                Err(e) => {
                    // Transient: expired tokens are already rotated inside the
                    // session, so this is a network/backend blip. Retry next tick.
                    tracing::debug!(error = %e, "polling for pending sessions failed");
                    continue;
                }
            };

            for ps in pending {
                let session_id = ps.session.id.clone();
                {
                    // Answer each session at most once. The session stays
                    // "connecting" in the backend until the controller ends it or
                    // it times out server-side; without this guard we would
                    // reconnect every poll, and each reconnect makes the mobile
                    // peer see the agent leave/rejoin — which tore down the call.
                    let mut guard = active.lock().await;
                    if guard.contains(&session_id) {
                        continue;
                    }
                    guard.insert(session_id.clone());
                    // Bound memory on a long-lived daemon; session ids are UUIDs
                    // and old ones are never revisited by the backend.
                    if guard.len() > 512 {
                        guard.clear();
                        guard.insert(session_id.clone());
                    }
                }
                tracing::info!(session_id = %session_id, "answering incoming session");
                let this = Arc::clone(&self);
                tokio::spawn(async move {
                    // Held for the whole session so `status` counts it, however
                    // the task ends.
                    let _tracked = this.state.track_session();
                    if let Err(e) = this.handle_session(ps).await {
                        tracing::warn!(session_id = %session_id, error = %e, "session ended with error");
                        this.state.record_error(format!("session {session_id} failed: {e}"));
                    } else {
                        tracing::info!(session_id = %session_id, "session ended");
                    }
                });
            }
        }
    }

    /// Handle one session end-to-end: signaling + WebRTC answer + streaming.
    async fn handle_session(&self, ps: PendingSession) -> anyhow::Result<()> {
        let session_id = ps.session.id.clone();

        let signaling = Arc::new(WebSocketSignaling::new(
            &ps.signaling_url,
            session_id.clone(),
            &ps.signaling_ticket,
            "agent",
        ));
        signaling
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("signaling connect: {e}"))?;

        let ice_servers = ps
            .ice_servers
            .into_iter()
            .map(|s| IceServer {
                urls: s.urls,
                username: s.username,
                credential: s.credential,
            })
            .collect();
        let (peer, mut events) = AgentPeer::new(PeerConfig { ice_servers }).await?;
        let peer = Arc::new(peer);

        // Task: forward peer events (local ICE -> signaling; input/control ->
        // injector/dev-tools; state changes -> logs).
        let event_task = {
            let signaling = Arc::clone(&signaling);
            let input = Arc::clone(&self.input);
            let devtools = Arc::clone(&self.devtools);
            let session_id = session_id.clone();
            tokio::spawn(async move {
                while let Some(ev) = events.recv().await {
                    match ev {
                        PeerEvent::LocalIce {
                            candidate,
                            sdp_mline_index,
                        } => {
                            let nonce = signaling.next_nonce();
                            let _ = signaling
                                .send(SignalEnvelope::new(
                                    session_id.as_str(),
                                    nonce,
                                    SignalPayload::IceCandidate {
                                        candidate,
                                        sdp_m_line_index: sdp_mline_index,
                                    },
                                ))
                                .await;
                        }
                        PeerEvent::InputFrame(frame) => {
                            input.handle_frame(&frame).await;
                        }
                        PeerEvent::ControlFrame(frame) => {
                            let _ = devtools.handle_frame(&frame).await;
                        }
                        PeerEvent::StateChanged(state) => {
                            tracing::info!(state = %state, "peer connection state");
                        }
                    }
                }
            })
        };

        // Task: pump encoded screen frames to the peer.
        let frame_task = {
            let capture = Arc::clone(&self.capture);
            let peer = Arc::clone(&peer);
            tokio::spawn(async move {
                let encoder = JpegScreenEncoder::streaming_default();
                let mut rx = capture.subscribe();
                let mut last = Instant::now() - FRAME_INTERVAL;
                let mut sent: u64 = 0;
                let mut dropped: u64 = 0;
                loop {
                    if rx.changed().await.is_err() {
                        break;
                    }
                    // Always mark the latest frame seen to avoid a busy loop.
                    let frame = rx.borrow_and_update().clone();
                    if last.elapsed() < FRAME_INTERVAL {
                        continue;
                    }
                    let Some(frame) = frame else { continue };
                    // Skip the expensive JPEG encode when no controller is
                    // attached to this session's frames channel yet. Without this,
                    // every answered-but-idle session (e.g. stale sessions from
                    // earlier attempts) would encode at full frame rate and starve
                    // the CPU, throttling the session that IS connected.
                    if !peer.frames_open().await {
                        continue;
                    }
                    last = Instant::now();
                    let enc = encoder.clone();
                    // JPEG encoding is CPU-bound: keep it off the async runtime.
                    match tokio::task::spawn_blocking(move || enc.encode(&frame)).await {
                        Ok(Ok(encoded)) => {
                            let bytes = encoded.data.len();
                            match peer.send_frame(&encoded, MAX_BUFFERED).await {
                                Ok(true) => sent += 1,
                                Ok(false) => dropped += 1,
                                Err(e) => {
                                    tracing::warn!(error = %e, "frame send failed");
                                    dropped += 1;
                                }
                            }
                            // Periodically surface streaming health so we can tell
                            // whether frames actually reach the peer (sent) versus
                            // being dropped because the channel is closed/backpressured.
                            if (sent + dropped) % 30 == 0 {
                                tracing::info!(sent, dropped, last_frame_bytes = bytes, "frame stream stats");
                            }
                        }
                        Ok(Err(e)) => tracing::debug!(error = %e, "frame encode failed"),
                        Err(_) => break,
                    }
                }
            })
        };

        // Drive the negotiation from inbound signaling until the session closes.
        let mut negotiation = NegotiationState::new(NegotiationRole::Agent);
        let result = self
            .signaling_loop(&signaling, &peer, &mut negotiation, &session_id)
            .await;

        // Teardown.
        frame_task.abort();
        event_task.abort();
        peer.close().await;
        result
    }

    async fn signaling_loop(
        &self,
        signaling: &Arc<WebSocketSignaling>,
        peer: &Arc<AgentPeer>,
        negotiation: &mut NegotiationState,
        session_id: &str,
    ) -> anyhow::Result<()> {
        loop {
            match signaling.recv().await {
                Ok(Some(env)) => match negotiation.on_signal(&env.payload) {
                    NegotiationAction::CreateAnswer { sdp } => {
                        let answer = peer.accept_offer(&sdp).await?;
                        let nonce = signaling.next_nonce();
                        signaling
                            .send(SignalEnvelope::new(
                                session_id,
                                nonce,
                                SignalPayload::Answer { sdp: answer },
                            ))
                            .await
                            .map_err(|e| anyhow::anyhow!("send answer: {e}"))?;
                    }
                    NegotiationAction::AddIceCandidate {
                        candidate,
                        sdp_m_line_index,
                    } => {
                        if let Err(e) = peer.add_remote_ice(candidate, sdp_m_line_index).await {
                            tracing::debug!(error = %e, "failed to add remote ICE candidate");
                        }
                    }
                    NegotiationAction::Close => return Ok(()),
                    _ => {}
                },
                Ok(None) => return Ok(()),
                Err(e) => return Err(anyhow::anyhow!("signaling recv: {e}")),
            }
        }
    }
}
