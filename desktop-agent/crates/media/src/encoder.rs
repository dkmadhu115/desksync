//! Baseline JPEG screen encoder.
//!
//! The capture pipeline produces raw BGRA [`Frame`]s. For the data-channel
//! frame stream we encode each frame to a self-contained JPEG: it needs no
//! inter-frame state, survives packet loss trivially (every frame is a
//! keyframe), and uses a pure-Rust encoder with no system libraries — so it
//! builds and is unit-tested everywhere. A hardware H.264/VP9 track is a future
//! optimization; MJPEG-over-datachannel is the robust baseline.
//!
//! Frames are re-bounded to a streaming height and quality that keep a single
//! encoded frame comfortably under the WebRTC data-channel message-size limit.

use desksync_capture::frame::downscale_to_max_height;
use desksync_capture::Frame;
use jpeg_encoder::{ColorType, Encoder};

/// Errors from encoding a frame.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// The frame's buffer length did not match its declared dimensions.
    #[error("invalid frame buffer")]
    InvalidFrame,
    /// The underlying JPEG encoder failed.
    #[error("jpeg encode: {0}")]
    Encode(String),
}

/// A JPEG-encoded frame ready to send over the data channel.
#[derive(Debug, Clone)]
pub struct EncodedFrame {
    /// Encoded frame width in pixels.
    pub width: u32,
    /// Encoded frame height in pixels.
    pub height: u32,
    /// Monotonic capture timestamp (microseconds) carried from the source.
    pub timestamp_us: u64,
    /// JPEG (JFIF) byte stream.
    pub data: Vec<u8>,
}

/// Encodes BGRA frames to JPEG, bounding resolution and quality for streaming.
#[derive(Debug, Clone)]
pub struct JpegScreenEncoder {
    max_height: u32,
    quality: u8,
}

impl JpegScreenEncoder {
    /// Build an encoder that caps frames at `max_height` px and encodes at
    /// `quality` (1–100). Values are clamped to sane bounds.
    pub fn new(max_height: u32, quality: u8) -> Self {
        Self {
            max_height: max_height.clamp(120, 2160),
            quality: quality.clamp(20, 90),
        }
    }

    /// Streaming-friendly default: 720p, moderate quality.
    pub fn streaming_default() -> Self {
        Self::new(720, 55)
    }

    /// Encode one BGRA frame to JPEG, downscaling to the streaming height first.
    pub fn encode(&self, frame: &Frame) -> Result<EncodedFrame, EncodeError> {
        let scaled = downscale_to_max_height(frame, self.max_height);
        if !scaled.is_valid() {
            return Err(EncodeError::InvalidFrame);
        }
        let mut buf = Vec::with_capacity(64 * 1024);
        let encoder = Encoder::new(&mut buf, self.quality);
        encoder
            .encode(&scaled.data, scaled.width as u16, scaled.height as u16, ColorType::Bgra)
            .map_err(|e| EncodeError::Encode(e.to_string()))?;
        Ok(EncodedFrame {
            width: scaled.width,
            height: scaled.height,
            timestamp_us: scaled.timestamp_us,
            data: buf,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(width: u32, height: u32, px: [u8; 4]) -> Frame {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            data.extend_from_slice(&px);
        }
        Frame {
            width,
            height,
            timestamp_us: 42,
            data,
        }
    }

    #[test]
    fn encodes_valid_jpeg_with_soi_marker() {
        let enc = JpegScreenEncoder::streaming_default();
        let frame = solid(320, 240, [10, 120, 200, 255]);
        let out = enc.encode(&frame).expect("encode");
        assert_eq!((out.width, out.height), (320, 240));
        assert_eq!(out.timestamp_us, 42);
        // JPEG streams begin with the SOI marker 0xFFD8 and end with EOI 0xFFD9.
        assert_eq!(&out.data[0..2], &[0xFF, 0xD8]);
        assert_eq!(&out.data[out.data.len() - 2..], &[0xFF, 0xD9]);
    }

    #[test]
    fn downscales_to_streaming_height() {
        let enc = JpegScreenEncoder::new(720, 50);
        let frame = solid(3840, 2160, [0, 0, 0, 255]);
        let out = enc.encode(&frame).expect("encode");
        assert_eq!(out.height, 720);
        assert_eq!(out.width, 1280);
    }

    #[test]
    fn rejects_invalid_buffer() {
        let enc = JpegScreenEncoder::streaming_default();
        let bad = Frame {
            width: 10,
            height: 10,
            timestamp_us: 0,
            data: vec![0u8; 8], // too short
        };
        assert!(matches!(enc.encode(&bad), Err(EncodeError::InvalidFrame)));
    }
}
