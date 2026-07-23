//! Pure WebRTC negotiation state machine.
//!
//! This models the offer/answer/ICE handshake as a deterministic, side-effect
//! -free state machine so it can be unit-tested exhaustively without a real
//! peer connection or network. The runtime feeds it signaling events and peer
//! presence and executes the [`NegotiationAction`] it returns (create an offer,
//! create an answer, apply a remote description, add an ICE candidate, or tear
//! down). The desktop agent is the **answerer** (`Agent`); the mobile client is
//! the **offerer** (`Controller`) — whichever side is the offerer creates the
//! offer once both peers are present.

use crate::SignalPayload;

/// Which side of the negotiation this endpoint plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationRole {
    /// The offerer (mobile controller): creates the SDP offer.
    Controller,
    /// The answerer (desktop agent): answers the offer.
    Agent,
}

impl NegotiationRole {
    fn is_offerer(self) -> bool {
        matches!(self, NegotiationRole::Controller)
    }
}

/// The negotiation phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Connected to signaling; waiting for the other peer to appear.
    WaitingForPeer,
    /// Both peers present; the offerer should create/serve an offer.
    ReadyToNegotiate,
    /// An offer has been created/received; waiting for the answer (offerer) or
    /// waiting to produce it (answerer).
    Offered,
    /// SDP exchange complete; exchanging ICE candidates / establishing.
    Connecting,
    /// Media/data path established.
    Connected,
    /// Session torn down.
    Closed,
}

/// The action the runtime should perform after feeding an event in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationAction {
    /// Do nothing.
    None,
    /// Create an SDP offer and send it (offerer only).
    CreateOffer,
    /// Apply the remote offer and create an answer (answerer only).
    CreateAnswer {
        /// The remote SDP offer.
        sdp: String,
    },
    /// Apply the remote answer to the local peer connection (offerer only).
    ApplyAnswer {
        /// The remote SDP answer.
        sdp: String,
    },
    /// Add a remote ICE candidate to the peer connection.
    AddIceCandidate {
        /// The candidate line.
        candidate: String,
        /// The media line index.
        sdp_m_line_index: u16,
    },
    /// Tear down the peer connection and close.
    Close,
}

/// The negotiation state machine.
#[derive(Debug, Clone)]
pub struct NegotiationState {
    role: NegotiationRole,
    phase: Phase,
    peer_present: bool,
}

impl NegotiationState {
    /// Create a fresh state machine for the given role, waiting for the peer.
    pub fn new(role: NegotiationRole) -> Self {
        Self {
            role,
            phase: Phase::WaitingForPeer,
            peer_present: false,
        }
    }

    /// Current phase.
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// Whether the other peer is currently present.
    pub fn peer_present(&self) -> bool {
        self.peer_present
    }

    /// Feed a decoded signaling payload; returns the action to perform.
    pub fn on_signal(&mut self, payload: &SignalPayload) -> NegotiationAction {
        match payload {
            SignalPayload::PeerJoined { .. } => self.on_peer_joined(),
            SignalPayload::PeerLeft { .. } | SignalPayload::Bye => self.close(),
            SignalPayload::Offer { sdp } => self.on_offer(sdp),
            SignalPayload::Answer { sdp } => self.on_answer(sdp),
            SignalPayload::IceCandidate {
                candidate,
                sdp_m_line_index,
            } => self.on_ice(candidate, *sdp_m_line_index),
            SignalPayload::Heartbeat => NegotiationAction::None,
        }
    }

    /// Signal that the local ICE agent reached a connected state.
    pub fn on_connected(&mut self) {
        if self.phase != Phase::Closed {
            self.phase = Phase::Connected;
        }
    }

    /// Force-close the negotiation.
    pub fn close(&mut self) -> NegotiationAction {
        if self.phase == Phase::Closed {
            return NegotiationAction::None;
        }
        self.phase = Phase::Closed;
        self.peer_present = false;
        NegotiationAction::Close
    }

    fn on_peer_joined(&mut self) -> NegotiationAction {
        if self.phase == Phase::Closed {
            return NegotiationAction::None;
        }
        self.peer_present = true;
        // Only advance from the initial wait; ignore duplicate presence.
        if self.phase == Phase::WaitingForPeer {
            self.phase = Phase::ReadyToNegotiate;
            if self.role.is_offerer() {
                self.phase = Phase::Offered;
                return NegotiationAction::CreateOffer;
            }
        }
        NegotiationAction::None
    }

    fn on_offer(&mut self, sdp: &str) -> NegotiationAction {
        // Only the answerer acts on an offer.
        if self.role.is_offerer() || self.phase == Phase::Closed {
            return NegotiationAction::None;
        }
        self.phase = Phase::Connecting;
        NegotiationAction::CreateAnswer { sdp: sdp.to_string() }
    }

    fn on_answer(&mut self, sdp: &str) -> NegotiationAction {
        // Only the offerer acts on an answer, and only after offering.
        if !self.role.is_offerer() || self.phase != Phase::Offered {
            return NegotiationAction::None;
        }
        self.phase = Phase::Connecting;
        NegotiationAction::ApplyAnswer { sdp: sdp.to_string() }
    }

    fn on_ice(&mut self, candidate: &str, idx: u16) -> NegotiationAction {
        // Accept trickled candidates once we are negotiating or connecting.
        match self.phase {
            Phase::Closed | Phase::WaitingForPeer => NegotiationAction::None,
            _ => NegotiationAction::AddIceCandidate {
                candidate: candidate.to_string(),
                sdp_m_line_index: idx,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer_joined(role: &str) -> SignalPayload {
        SignalPayload::PeerJoined { role: role.to_string() }
    }

    #[test]
    fn controller_offers_when_peer_joins() {
        let mut s = NegotiationState::new(NegotiationRole::Controller);
        assert_eq!(s.phase(), Phase::WaitingForPeer);
        let action = s.on_signal(&peer_joined("agent"));
        assert_eq!(action, NegotiationAction::CreateOffer);
        assert_eq!(s.phase(), Phase::Offered);
    }

    #[test]
    fn agent_waits_then_answers_offer() {
        let mut s = NegotiationState::new(NegotiationRole::Agent);
        // Presence does not make the agent offer.
        assert_eq!(s.on_signal(&peer_joined("controller")), NegotiationAction::None);
        assert_eq!(s.phase(), Phase::ReadyToNegotiate);

        let action = s.on_signal(&SignalPayload::Offer { sdp: "v=0".into() });
        assert_eq!(action, NegotiationAction::CreateAnswer { sdp: "v=0".into() });
        assert_eq!(s.phase(), Phase::Connecting);
    }

    #[test]
    fn controller_applies_answer_only_after_offering() {
        let mut s = NegotiationState::new(NegotiationRole::Controller);
        // Answer before offering is ignored.
        assert_eq!(
            s.on_signal(&SignalPayload::Answer { sdp: "a".into() }),
            NegotiationAction::None
        );
        s.on_signal(&peer_joined("agent")); // -> CreateOffer, Offered
        let action = s.on_signal(&SignalPayload::Answer { sdp: "ans".into() });
        assert_eq!(action, NegotiationAction::ApplyAnswer { sdp: "ans".into() });
        assert_eq!(s.phase(), Phase::Connecting);
    }

    #[test]
    fn agent_ignores_answer_and_controller_ignores_offer() {
        let mut agent = NegotiationState::new(NegotiationRole::Agent);
        agent.on_signal(&peer_joined("controller"));
        assert_eq!(
            agent.on_signal(&SignalPayload::Answer { sdp: "x".into() }),
            NegotiationAction::None
        );

        let mut ctrl = NegotiationState::new(NegotiationRole::Controller);
        ctrl.on_signal(&peer_joined("agent"));
        assert_eq!(
            ctrl.on_signal(&SignalPayload::Offer { sdp: "x".into() }),
            NegotiationAction::None
        );
    }

    #[test]
    fn ice_candidates_accepted_while_connecting_not_before_peer() {
        let mut s = NegotiationState::new(NegotiationRole::Agent);
        // Before a peer, ICE is dropped.
        assert_eq!(
            s.on_signal(&SignalPayload::IceCandidate {
                candidate: "c".into(),
                sdp_m_line_index: 0
            }),
            NegotiationAction::None
        );
        s.on_signal(&peer_joined("controller"));
        s.on_signal(&SignalPayload::Offer { sdp: "v=0".into() });
        let action = s.on_signal(&SignalPayload::IceCandidate {
            candidate: "cand".into(),
            sdp_m_line_index: 1,
        });
        assert_eq!(
            action,
            NegotiationAction::AddIceCandidate {
                candidate: "cand".into(),
                sdp_m_line_index: 1
            }
        );
    }

    #[test]
    fn peer_left_or_bye_closes() {
        let mut s = NegotiationState::new(NegotiationRole::Controller);
        s.on_signal(&peer_joined("agent"));
        assert_eq!(
            s.on_signal(&SignalPayload::PeerLeft { role: "agent".into() }),
            NegotiationAction::Close
        );
        assert_eq!(s.phase(), Phase::Closed);
        // Idempotent close.
        assert_eq!(s.close(), NegotiationAction::None);
    }

    #[test]
    fn duplicate_presence_does_not_reoffer() {
        let mut s = NegotiationState::new(NegotiationRole::Controller);
        assert_eq!(s.on_signal(&peer_joined("agent")), NegotiationAction::CreateOffer);
        // A second peer_joined must not create another offer.
        assert_eq!(s.on_signal(&peer_joined("agent")), NegotiationAction::None);
    }

    #[test]
    fn on_connected_marks_connected() {
        let mut s = NegotiationState::new(NegotiationRole::Agent);
        s.on_signal(&peer_joined("controller"));
        s.on_signal(&SignalPayload::Offer { sdp: "v=0".into() });
        s.on_connected();
        assert_eq!(s.phase(), Phase::Connected);
    }
}
