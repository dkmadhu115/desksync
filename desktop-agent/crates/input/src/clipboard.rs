//! Clipboard synchronization abstraction.
//!
//! Clipboard sharing is bidirectional: text set on the mobile client is pushed
//! to the host (via an [`crate::InputEvent::ClipboardText`] event handled by the
//! injector) and the host clipboard can be read back to the client. The
//! [`Clipboard`] trait hides the OS backend; [`NoopClipboard`] provides an
//! in-memory implementation for tests, and `ArboardClipboard` (behind the
//! `native` feature) uses the real OS clipboard.

use async_trait::async_trait;
use desksync_core::error::Result;
use std::sync::Mutex;

/// Backend-agnostic clipboard access.
#[async_trait]
pub trait Clipboard: Send + Sync {
    /// Read the current clipboard text, if any.
    async fn get_text(&self) -> Result<Option<String>>;
    /// Replace the clipboard text.
    async fn set_text(&self, text: &str) -> Result<()>;
}

/// An in-memory clipboard used in tests and headless environments.
#[derive(Debug, Default)]
pub struct NoopClipboard {
    contents: Mutex<Option<String>>,
}

impl NoopClipboard {
    /// Create an empty in-memory clipboard.
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Clipboard for NoopClipboard {
    async fn get_text(&self) -> Result<Option<String>> {
        Ok(self.contents.lock().expect("clipboard mutex").clone())
    }
    async fn set_text(&self, text: &str) -> Result<()> {
        *self.contents.lock().expect("clipboard mutex") = Some(text.to_owned());
        Ok(())
    }
}

/// The real OS clipboard, backed by `arboard`.
#[cfg(feature = "native")]
pub struct ArboardClipboard;

#[cfg(feature = "native")]
impl ArboardClipboard {
    /// Create a handle to the OS clipboard.
    pub fn new() -> Self {
        Self
    }
}

#[cfg(feature = "native")]
impl Default for ArboardClipboard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(feature = "native")]
#[async_trait]
impl Clipboard for ArboardClipboard {
    async fn get_text(&self) -> Result<Option<String>> {
        // `arboard::Clipboard` is not `Send`, so it is created and used entirely
        // within this synchronous section (no `.await` while it is alive).
        let mut cb = arboard::Clipboard::new()
            .map_err(|e| desksync_core::error::AgentError::subsystem("clipboard", format!("open: {e}")))?;
        match cb.get_text() {
            Ok(text) => Ok(Some(text)),
            // An empty/non-text clipboard is not an error.
            Err(arboard::Error::ContentNotAvailable) => Ok(None),
            Err(e) => Err(desksync_core::error::AgentError::subsystem(
                "clipboard",
                format!("read: {e}"),
            )),
        }
    }
    async fn set_text(&self, text: &str) -> Result<()> {
        let mut cb = arboard::Clipboard::new()
            .map_err(|e| desksync_core::error::AgentError::subsystem("clipboard", format!("open: {e}")))?;
        cb.set_text(text.to_owned())
            .map_err(|e| desksync_core::error::AgentError::subsystem("clipboard", format!("write: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn noop_clipboard_roundtrips() {
        let cb = NoopClipboard::new();
        assert_eq!(cb.get_text().await.unwrap(), None);
        cb.set_text("hello").await.unwrap();
        assert_eq!(cb.get_text().await.unwrap(), Some("hello".to_string()));
        cb.set_text("world").await.unwrap();
        assert_eq!(cb.get_text().await.unwrap(), Some("world".to_string()));
    }
}
