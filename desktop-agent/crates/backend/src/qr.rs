//! Render a pairing payload as a terminal-scannable Unicode QR code.

use qrcode::render::unicode;
use qrcode::QrCode;

use crate::error::{BackendError, Result};

/// Render `payload` as a Unicode (half-block) QR code string suitable for
/// printing to a terminal. The mobile app scans it directly off the screen.
pub fn render_qr(payload: &str) -> Result<String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| BackendError::Decode(format!("qr encode: {e}")))?;
    Ok(code
        .render::<unicode::Dense1x2>()
        .dark_color(unicode::Dense1x2::Light)
        .light_color(unicode::Dense1x2::Dark)
        .quiet_zone(true)
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_non_empty_block_string() {
        let out = render_qr("desksync://pair?v=1&pid=abc&code=12345678").unwrap();
        assert!(!out.is_empty());
        // The Unicode renderer emits half-block glyphs.
        assert!(out.contains('▀') || out.contains('▄') || out.contains('█') || out.contains(' '));
        // Multi-line output (a QR grid).
        assert!(out.lines().count() > 5);
    }

    #[test]
    fn deterministic_for_same_payload() {
        let a = render_qr("desksync://pair?pid=x&code=1").unwrap();
        let b = render_qr("desksync://pair?pid=x&code=1").unwrap();
        assert_eq!(a, b);
    }
}
