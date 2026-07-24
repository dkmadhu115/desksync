//! Data-channel control-frame router.
//!
//! The mobile controller sends input over the WebRTC input data channel as one
//! JSON [`InputEvent`] per text frame (see the wire contract in this crate's
//! `lib.rs`). [`InputRouter`] decodes each frame and dispatches it:
//!
//! - pointer/key events go to the [`InputInjector`];
//! - [`InputEvent::ClipboardText`] is written to the OS [`Clipboard`] *and*
//!   forwarded to the injector (so a `native` injector that handles clipboard
//!   itself stays consistent).
//!
//! Frames that fail to parse are rejected without disturbing the session: a
//! hostile or buggy peer cannot crash the agent by sending garbage. This module
//! is pure Rust (no platform APIs) and fully unit-tested; it is driven by the
//! WebRTC peer's data-channel `on_message` callback in the `native` build.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::clipboard::Clipboard;
use crate::{InputEvent, InputInjector};

/// Outcome of handling a single control frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOutcome {
    /// The frame was decoded and dispatched successfully.
    Dispatched,
    /// The frame could not be decoded as an [`InputEvent`].
    Rejected,
}

/// Routes decoded control frames to the input injector and clipboard.
pub struct InputRouter {
    injector: Arc<dyn InputInjector>,
    clipboard: Arc<dyn Clipboard>,
    dispatched: AtomicU64,
    rejected: AtomicU64,
}

impl InputRouter {
    /// Build a router over the given injector and clipboard.
    pub fn new(injector: Arc<dyn InputInjector>, clipboard: Arc<dyn Clipboard>) -> Self {
        Self {
            injector,
            clipboard,
            dispatched: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    /// Number of frames dispatched successfully so far.
    pub fn dispatched(&self) -> u64 {
        self.dispatched.load(Ordering::SeqCst)
    }

    /// Number of frames rejected (malformed) so far.
    pub fn rejected(&self) -> u64 {
        self.rejected.load(Ordering::SeqCst)
    }

    /// Decode and dispatch a single JSON control frame.
    ///
    /// Returns [`FrameOutcome::Rejected`] (never an error) for frames that do
    /// not parse, so the caller's receive loop keeps running. Injection errors
    /// (e.g. out-of-range coordinates) are surfaced so the caller can log them,
    /// but the frame still counts as handled.
    pub async fn handle_frame(&self, frame: &str) -> FrameOutcome {
        let event = match serde_json::from_str::<InputEvent>(frame) {
            Ok(ev) => ev,
            Err(e) => {
                self.rejected.fetch_add(1, Ordering::SeqCst);
                tracing::debug!(error = %e, "dropping malformed input frame");
                return FrameOutcome::Rejected;
            }
        };
        self.dispatch(event).await;
        self.dispatched.fetch_add(1, Ordering::SeqCst);
        FrameOutcome::Dispatched
    }

    /// Dispatch an already-decoded event.
    pub async fn dispatch(&self, event: InputEvent) {
        // Mirror clipboard writes to the OS clipboard so a paste on the desktop
        // reflects what was sent from the phone.
        if let InputEvent::ClipboardText { text } = &event {
            if let Err(e) = self.clipboard.set_text(text).await {
                tracing::warn!(error = %e, "failed to set OS clipboard");
            }
        }
        if let Err(e) = self.injector.inject(event).await {
            tracing::warn!(error = %e, "input injection failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::NoopClipboard;
    use crate::{MouseButton, NoopInjector};

    fn router() -> (InputRouter, Arc<NoopInjector>, Arc<NoopClipboard>) {
        let injector = Arc::new(NoopInjector::new());
        let clipboard = Arc::new(NoopClipboard::new());
        let router = InputRouter::new(injector.clone(), clipboard.clone());
        (router, injector, clipboard)
    }

    #[tokio::test]
    async fn dispatches_pointer_and_key_frames() {
        let (router, injector, _) = router();

        assert_eq!(
            router.handle_frame(r#"{"type":"mouse_move","x":0.5,"y":0.5}"#).await,
            FrameOutcome::Dispatched
        );
        assert_eq!(
            router
                .handle_frame(r#"{"type":"mouse_button","button":"left","pressed":true,"modifiers":{"ctrl":false,"alt":false,"shift":false,"meta":false}}"#)
                .await,
            FrameOutcome::Dispatched
        );
        assert_eq!(
            router.handle_frame(r#"{"type":"key","code":65,"pressed":true}"#).await,
            FrameOutcome::Dispatched
        );

        assert_eq!(injector.count(), 3);
        assert_eq!(router.dispatched(), 3);
        assert_eq!(router.rejected(), 0);
    }

    #[tokio::test]
    async fn clipboard_frame_writes_os_clipboard_and_injects() {
        let (router, injector, clipboard) = router();

        let outcome = router
            .handle_frame(r#"{"type":"clipboard_text","text":"hello world"}"#)
            .await;

        assert_eq!(outcome, FrameOutcome::Dispatched);
        assert_eq!(clipboard.get_text().await.unwrap(), Some("hello world".to_string()));
        assert_eq!(injector.count(), 1);
    }

    #[tokio::test]
    async fn malformed_frames_are_rejected_without_injecting() {
        let (router, injector, _) = router();

        assert_eq!(router.handle_frame("not json").await, FrameOutcome::Rejected);
        assert_eq!(router.handle_frame(r#"{"type":"nope"}"#).await, FrameOutcome::Rejected);
        assert_eq!(router.handle_frame(r#"{"x":1}"#).await, FrameOutcome::Rejected);

        assert_eq!(injector.count(), 0);
        assert_eq!(router.rejected(), 3);
        assert_eq!(router.dispatched(), 0);
    }

    #[tokio::test]
    async fn out_of_range_pointer_counts_as_handled_but_not_injected() {
        let (router, injector, _) = router();

        // The frame is well-formed, so it is "dispatched"; the injector rejects
        // the out-of-range coordinate internally without incrementing its count.
        let outcome = router.handle_frame(r#"{"type":"mouse_move","x":9.0,"y":0.0}"#).await;
        assert_eq!(outcome, FrameOutcome::Dispatched);
        assert_eq!(injector.count(), 0);
    }

    #[tokio::test]
    async fn dispatch_accepts_prebuilt_events() {
        let (router, injector, _) = router();
        router
            .dispatch(InputEvent::MouseButton {
                button: MouseButton::Right,
                pressed: false,
                modifiers: Default::default(),
            })
            .await;
        assert_eq!(injector.count(), 1);
    }
}
