//! Media pipeline for the DeskSync desktop agent.
//!
//! Two pieces live here:
//!
//! - [`encoder`]: a pure-Rust BGRA→JPEG screen encoder (always compiled and
//!   unit-tested).
//! - [`rtc`]: the WebRTC **answerer** peer that establishes the connection to
//!   the mobile controller (ICE/DTLS/SCTP), receives the input/control data
//!   channels, and streams encoded frames back over a data channel. It pulls in
//!   the (pure-Rust) `webrtc` stack and is gated behind the `rtc` feature so
//!   default/headless builds stay lean.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod encoder;

pub use encoder::{EncodeError, EncodedFrame, JpegScreenEncoder};

#[cfg(feature = "rtc")]
pub mod rtc;

#[cfg(feature = "rtc")]
pub use rtc::{AgentPeer, IceServer, PeerConfig, PeerEvent};
