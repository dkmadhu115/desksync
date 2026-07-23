//! Input injection abstraction.
//!
//! Translates high-level input events received from the mobile client into
//! host OS input events (Windows SendInput, macOS CGEvent, Linux uinput/XTest).
//! This crate defines the event model, the [`InputInjector`] trait with a
//! [`NoopInjector`], pure coordinate/keycode [`mapping`], a [`clipboard`]
//! abstraction, and the native `enigo`/`arboard` backend (behind the `native`
//! feature).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use async_trait::async_trait;
use desksync_core::error::Result;
use desksync_core::subsystem::{HealthStatus, Subsystem};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub mod clipboard;
pub mod mapping;

#[cfg(feature = "native")]
pub mod native;

pub use clipboard::{Clipboard, NoopClipboard};

#[cfg(feature = "native")]
pub use native::EnigoInjector;

/// Modifier keys that can accompany a key or pointer event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    /// Control key held.
    pub ctrl: bool,
    /// Alt/Option key held.
    pub alt: bool,
    /// Shift key held.
    pub shift: bool,
    /// Command/Meta/Win key held.
    pub meta: bool,
}

/// Mouse buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle button.
    Middle,
}

/// A normalized input event. Coordinates are normalized to the range [0.0, 1.0]
/// relative to the captured monitor so they are resolution-independent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputEvent {
    /// Absolute pointer move.
    MouseMove {
        /// Normalized X in [0,1].
        x: f64,
        /// Normalized Y in [0,1].
        y: f64,
    },
    /// Pointer button press/release.
    MouseButton {
        /// Which button.
        button: MouseButton,
        /// True on press, false on release.
        pressed: bool,
        /// Active modifiers.
        #[serde(default)]
        modifiers: Modifiers,
    },
    /// Scroll wheel / trackpad scroll.
    Scroll {
        /// Horizontal delta.
        dx: f64,
        /// Vertical delta.
        dy: f64,
    },
    /// Key press/release identified by a platform-independent key code.
    Key {
        /// USB HID usage / virtual key code.
        code: u32,
        /// True on press, false on release.
        pressed: bool,
        /// Active modifiers.
        #[serde(default)]
        modifiers: Modifiers,
    },
    /// Direct clipboard text set from the mobile device.
    ClipboardText {
        /// UTF-8 clipboard contents.
        text: String,
    },
}

/// Backend-agnostic input injection interface.
#[async_trait]
pub trait InputInjector: Subsystem {
    /// Inject a single input event into the host OS.
    async fn inject(&self, event: InputEvent) -> Result<()>;
}

/// A no-op injector that validates and counts events without touching the OS.
#[derive(Debug, Default)]
pub struct NoopInjector {
    injected: AtomicU64,
}

impl NoopInjector {
    /// Create a new no-op injector.
    pub fn new() -> Self {
        Self::default()
    }
    /// Number of events injected so far.
    pub fn count(&self) -> u64 {
        self.injected.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Subsystem for NoopInjector {
    fn name(&self) -> &'static str {
        "input"
    }
    async fn start(&self) -> Result<()> {
        Ok(())
    }
    async fn stop(&self) -> Result<()> {
        Ok(())
    }
    async fn health(&self) -> HealthStatus {
        HealthStatus::Healthy
    }
}

#[async_trait]
impl InputInjector for NoopInjector {
    async fn inject(&self, event: InputEvent) -> Result<()> {
        // Guard against out-of-range coordinates before a real backend would
        // forward them to the OS.
        if let InputEvent::MouseMove { x, y } = event {
            if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
                return Err(desksync_core::error::AgentError::subsystem(
                    "input",
                    "mouse coordinates out of normalized range",
                ));
            }
        }
        self.injected.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn injects_and_counts_events() {
        let i = NoopInjector::new();
        i.inject(InputEvent::MouseMove { x: 0.5, y: 0.5 }).await.unwrap();
        i.inject(InputEvent::Key {
            code: 65,
            pressed: true,
            modifiers: Modifiers {
                ctrl: true,
                ..Default::default()
            },
        })
        .await
        .unwrap();
        assert_eq!(i.count(), 2);
    }

    #[tokio::test]
    async fn rejects_out_of_range_mouse() {
        let i = NoopInjector::new();
        assert!(i.inject(InputEvent::MouseMove { x: 1.5, y: 0.0 }).await.is_err());
        assert_eq!(i.count(), 0);
    }

    #[test]
    fn events_roundtrip_json() {
        let e = InputEvent::MouseButton {
            button: MouseButton::Right,
            pressed: true,
            modifiers: Modifiers::default(),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: InputEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(e, back);
    }
}
