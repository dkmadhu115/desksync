//! WebRTC **answerer** peer for the desktop agent.
//!
//! The mobile controller is the offerer: it creates the peer connection and the
//! data channels (`input`, `control`, and `frames`) and sends the SDP offer.
//! This peer answers: it applies the offer, produces an answer, trickles ICE
//! candidates, receives the input/control channels, and **sends encoded screen
//! frames back over the bidirectional `frames` channel**.
//!
//! Everything the surrounding runtime needs flows through an event channel
//! ([`PeerEvent`]): local ICE candidates to forward over signaling, decoded
//! input/control frames to dispatch, and connection-state transitions. This
//! keeps the peer decoupled from signaling, input injection, and dev-tools.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use bytes::Bytes;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::Mutex;

use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::data_channel_state::RTCDataChannelState;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_candidate::{RTCIceCandidate, RTCIceCandidateInit};
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::Registry;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use webrtc::peer_connection::RTCPeerConnection;

use crate::encoder::EncodedFrame;

/// A single ICE server (STUN/TURN) as returned by the session service.
#[derive(Debug, Clone)]
pub struct IceServer {
    /// STUN/TURN URLs (e.g. `stun:host:3478`, `turn:host:3478?transport=udp`).
    pub urls: Vec<String>,
    /// TURN username (empty for STUN).
    pub username: String,
    /// TURN credential (empty for STUN).
    pub credential: String,
}

/// Configuration for an [`AgentPeer`].
#[derive(Debug, Clone, Default)]
pub struct PeerConfig {
    /// ICE servers to use for connectivity (STUN + TURN relay fallback).
    pub ice_servers: Vec<IceServer>,
}

/// Events emitted by the peer for the runtime to act on.
#[derive(Debug, Clone)]
pub enum PeerEvent {
    /// A locally-gathered ICE candidate to forward to the peer via signaling.
    LocalIce {
        /// The candidate line.
        candidate: String,
        /// The media-line index.
        sdp_mline_index: u16,
    },
    /// A decoded input frame (JSON) received on the `input` data channel.
    InputFrame(String),
    /// A decoded control frame (JSON) received on the `control` data channel.
    ControlFrame(String),
    /// The peer connection state changed (webrtc state name, lowercased).
    StateChanged(String),
}

/// Wire framing for chunked screen frames sent over the `frames` data channel.
///
/// A single encoded JPEG frame routinely exceeds the SCTP data channel's maximum
/// message size (the negotiated `a=max-message-size`), so sending a frame whole
/// fails with "outbound packet larger than maximum message size" and nothing
/// ever reaches the controller. We therefore split every frame into fixed-size
/// chunks, each prefixed with an 8-byte little-endian header so the controller
/// can reassemble them:
///
/// ```text
/// [ frame_id: u32 ][ chunk_index: u16 ][ chunk_count: u16 ][ payload… ]
/// ```
///
/// The `frames` channel is reliable+ordered, so chunks arrive in send order;
/// `frame_id` lets the controller detect frame boundaries and drop an incomplete
/// frame if a newer one starts (correct for live video — newest wins).
pub const FRAME_CHUNK_HEADER_LEN: usize = 8;

/// Payload bytes per chunk. Kept well under the widely-compatible 16 KiB data
/// channel message ceiling (header included) so a chunk never itself exceeds the
/// max message size on any peer.
pub const FRAME_CHUNK_PAYLOAD: usize = 16 * 1024 - FRAME_CHUNK_HEADER_LEN;

/// The WebRTC answerer peer.
pub struct AgentPeer {
    pc: Arc<RTCPeerConnection>,
    // The bidirectional `frames` channel (created by the controller); set once
    // it is announced via `on_data_channel`. The agent sends encoded frames on
    // it.
    frames: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    // Monotonic id stamped on every frame's chunks so the controller can tell
    // chunks of different frames apart and discard incomplete ones.
    frame_seq: AtomicU32,
}

impl AgentPeer {
    /// Build a peer from ICE configuration, returning the peer and a receiver of
    /// [`PeerEvent`]s. The peer is idle until [`accept_offer`](Self::accept_offer)
    /// is called with the controller's SDP offer.
    pub async fn new(config: PeerConfig) -> Result<(Self, UnboundedReceiver<PeerEvent>)> {
        let mut media = MediaEngine::default();
        media
            .register_default_codecs()
            .map_err(|e| anyhow!("register codecs: {e}"))?;

        let mut registry = Registry::new();
        registry = register_default_interceptors(registry, &mut media)
            .map_err(|e| anyhow!("register interceptors: {e}"))?;

        let api = APIBuilder::new()
            .with_media_engine(media)
            .with_interceptor_registry(registry)
            .build();

        let rtc_config = RTCConfiguration {
            ice_servers: config
                .ice_servers
                .into_iter()
                .map(|s| RTCIceServer {
                    urls: s.urls,
                    username: s.username,
                    credential: s.credential,
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };

        let pc = Arc::new(
            api.new_peer_connection(rtc_config)
                .await
                .map_err(|e| anyhow!("new peer connection: {e}"))?,
        );

        let (tx, rx) = mpsc::unbounded_channel::<PeerEvent>();
        let frames: Arc<Mutex<Option<Arc<RTCDataChannel>>>> = Arc::new(Mutex::new(None));

        Self::wire_ice(&pc, tx.clone());
        Self::wire_state(&pc, tx.clone());
        Self::wire_data_channels(&pc, tx, Arc::clone(&frames));

        Ok((
            Self {
                pc,
                frames,
                frame_seq: AtomicU32::new(0),
            },
            rx,
        ))
    }

    fn wire_ice(pc: &Arc<RTCPeerConnection>, tx: UnboundedSender<PeerEvent>) {
        pc.on_ice_candidate(Box::new(move |cand: Option<RTCIceCandidate>| {
            let tx = tx.clone();
            Box::pin(async move {
                let Some(cand) = cand else { return };
                match cand.to_json() {
                    Ok(init) => {
                        let _ = tx.send(PeerEvent::LocalIce {
                            candidate: init.candidate,
                            sdp_mline_index: init.sdp_mline_index.unwrap_or(0),
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "failed to serialize local ICE candidate"),
                }
            })
        }));
    }

    fn wire_state(pc: &Arc<RTCPeerConnection>, tx: UnboundedSender<PeerEvent>) {
        pc.on_peer_connection_state_change(Box::new(move |state: RTCPeerConnectionState| {
            let tx = tx.clone();
            Box::pin(async move {
                let name = match state {
                    RTCPeerConnectionState::New => "new",
                    RTCPeerConnectionState::Connecting => "connecting",
                    RTCPeerConnectionState::Connected => "connected",
                    RTCPeerConnectionState::Disconnected => "disconnected",
                    RTCPeerConnectionState::Failed => "failed",
                    RTCPeerConnectionState::Closed => "closed",
                    RTCPeerConnectionState::Unspecified => "unspecified",
                };
                let _ = tx.send(PeerEvent::StateChanged(name.to_string()));
            })
        }));
    }

    fn wire_data_channels(
        pc: &Arc<RTCPeerConnection>,
        tx: UnboundedSender<PeerEvent>,
        frames: Arc<Mutex<Option<Arc<RTCDataChannel>>>>,
    ) {
        pc.on_data_channel(Box::new(move |dc: Arc<RTCDataChannel>| {
            let label = dc.label().to_string();
            let tx = tx.clone();
            let frames = Arc::clone(&frames);
            Box::pin(async move {
                match label.as_str() {
                    "input" => wire_text_channel(&dc, tx, PeerEvent::InputFrame),
                    "control" => wire_text_channel(&dc, tx, PeerEvent::ControlFrame),
                    "frames" => {
                        // The agent SENDS on this channel; just retain it.
                        *frames.lock().await = Some(Arc::clone(&dc));
                        tracing::info!("frames data channel established");
                    }
                    other => tracing::debug!(label = other, "ignoring unknown data channel"),
                }
            })
        }));
    }

    /// Apply the controller's SDP offer and return the local SDP answer. ICE is
    /// trickled: local candidates arrive as [`PeerEvent::LocalIce`] afterwards.
    pub async fn accept_offer(&self, offer_sdp: &str) -> Result<String> {
        let offer = RTCSessionDescription::offer(offer_sdp.to_string())
            .map_err(|e| anyhow!("parse offer: {e}"))?;
        self.pc
            .set_remote_description(offer)
            .await
            .map_err(|e| anyhow!("set remote description: {e}"))?;

        let answer = self
            .pc
            .create_answer(None)
            .await
            .map_err(|e| anyhow!("create answer: {e}"))?;
        self.pc
            .set_local_description(answer)
            .await
            .map_err(|e| anyhow!("set local description: {e}"))?;

        self.pc
            .local_description()
            .await
            .map(|d| d.sdp)
            .ok_or_else(|| anyhow!("missing local description after answer"))
    }

    /// Add a remote ICE candidate received from the controller via signaling.
    pub async fn add_remote_ice(&self, candidate: String, sdp_mline_index: u16) -> Result<()> {
        self.pc
            .add_ice_candidate(RTCIceCandidateInit {
                candidate,
                sdp_mid: None,
                sdp_mline_index: Some(sdp_mline_index),
                username_fragment: None,
            })
            .await
            .map_err(|e| anyhow!("add ice candidate: {e}"))
    }

    /// Whether the `frames` channel exists and is open. Cheap check used to skip
    /// the (expensive) JPEG encode entirely for sessions that have no controller
    /// attached yet — critical when several stale sessions are answered at once,
    /// so they don't burn CPU encoding frames only to drop them.
    pub async fn frames_open(&self) -> bool {
        matches!(
            self.frames.lock().await.as_ref(),
            Some(dc) if dc.ready_state() == RTCDataChannelState::Open
        )
    }

    /// Send an encoded frame over the `frames` channel. Returns `Ok(false)` when
    /// the channel is not yet open or is backpressured (the frame is dropped —
    /// correct for live video, where the newest frame supersedes stale ones).
    pub async fn send_frame(&self, frame: &EncodedFrame, max_buffered: usize) -> Result<bool> {
        let guard = self.frames.lock().await;
        let Some(dc) = guard.as_ref() else {
            tracing::trace!("send_frame: frames channel not yet established");
            return Ok(false);
        };
        let state = dc.ready_state();
        if state != RTCDataChannelState::Open {
            tracing::trace!(?state, "send_frame: frames channel not open");
            return Ok(false);
        }
        if dc.buffered_amount().await > max_buffered {
            tracing::trace!("send_frame: backpressured, dropping frame");
            return Ok(false);
        }

        // Split the frame into chunks that each fit under the data channel's
        // maximum message size, prefixing every chunk with an 8-byte header so
        // the controller can reassemble them (see FRAME_CHUNK_* docs).
        let data = &frame.data;
        let chunk_count = data.len().div_ceil(FRAME_CHUNK_PAYLOAD).max(1);
        if chunk_count > u16::MAX as usize {
            return Err(anyhow!(
                "frame too large to chunk: {} bytes ({} chunks)",
                data.len(),
                chunk_count
            ));
        }
        let frame_id = self.frame_seq.fetch_add(1, Ordering::Relaxed);

        for (idx, payload) in data.chunks(FRAME_CHUNK_PAYLOAD).enumerate() {
            let mut msg = Vec::with_capacity(FRAME_CHUNK_HEADER_LEN + payload.len());
            msg.extend_from_slice(&frame_id.to_le_bytes());
            msg.extend_from_slice(&(idx as u16).to_le_bytes());
            msg.extend_from_slice(&(chunk_count as u16).to_le_bytes());
            msg.extend_from_slice(payload);
            dc.send(&Bytes::from(msg))
                .await
                .map_err(|e| anyhow!("send frame: {e}"))?;
        }
        Ok(true)
    }

    /// Close the peer connection.
    pub async fn close(&self) {
        let _ = self.pc.close().await;
    }
}

/// Wire a text data channel's `on_message` to emit the given [`PeerEvent`]
/// variant for each frame.
fn wire_text_channel<F>(dc: &Arc<RTCDataChannel>, tx: UnboundedSender<PeerEvent>, make: F)
where
    F: Fn(String) -> PeerEvent + Send + Sync + 'static,
{
    dc.on_message(Box::new(move |msg: DataChannelMessage| {
        let text = String::from_utf8_lossy(&msg.data).to_string();
        let ev = make(text);
        let tx = tx.clone();
        Box::pin(async move {
            let _ = tx.send(ev);
        })
    }));
}
