//! Frame geometry and raw pixel-scaling utilities.
//!
//! These are pure, allocation-only helpers (no platform APIs), so they are
//! fully unit-tested on every target. The real encoder lives in Phase 5; here
//! we only need to (a) pick sane output dimensions bounded by the configured
//! `max_height` and (b) downscale raw BGRA buffers when the source exceeds
//! that bound.

use crate::Frame;

/// Round a dimension down to the nearest even number (video encoders such as
/// H.264/VP9 require even width/height), with a floor of 2.
#[must_use]
pub fn make_even(v: u32) -> u32 {
    let e = v & !1;
    e.max(2)
}

/// Compute output dimensions for a source of `src_w`×`src_h`, bounded so the
/// height never exceeds `max_h`. Aspect ratio is preserved and the image is
/// only ever downscaled, never upscaled. Both returned dimensions are even.
#[must_use]
pub fn fit_dimensions(src_w: u32, src_h: u32, max_h: u32) -> (u32, u32) {
    if src_w == 0 || src_h == 0 {
        return (2, 2);
    }
    if src_h <= max_h {
        return (make_even(src_w), make_even(src_h));
    }
    let scale = f64::from(max_h) / f64::from(src_h);
    let w = (f64::from(src_w) * scale).round() as u32;
    (make_even(w), make_even(max_h))
}

/// Nearest-neighbour rescale of a raw BGRA frame to `dst_w`×`dst_h`.
///
/// Nearest-neighbour keeps this dependency-free and cheap; the production
/// pipeline hands frames to a hardware scaler/encoder in Phase 5, but this is a
/// correct, deterministic fallback and the reference used by tests.
#[must_use]
pub fn scale_bgra_nearest(src: &Frame, dst_w: u32, dst_h: u32) -> Frame {
    let dst_w = dst_w.max(1);
    let dst_h = dst_h.max(1);
    if src.width == dst_w && src.height == dst_h {
        return src.clone();
    }

    let mut data = vec![0u8; (dst_w as usize) * (dst_h as usize) * 4];
    let sw = src.width as usize;
    let sh = src.height as usize;

    for dy in 0..dst_h as usize {
        // Map destination row to the nearest source row.
        let sy = dy * sh / dst_h as usize;
        for dx in 0..dst_w as usize {
            let sx = dx * sw / dst_w as usize;
            let si = (sy * sw + sx) * 4;
            let di = (dy * dst_w as usize + dx) * 4;
            data[di..di + 4].copy_from_slice(&src.data[si..si + 4]);
        }
    }

    Frame {
        width: dst_w,
        height: dst_h,
        timestamp_us: src.timestamp_us,
        data,
    }
}

/// Downscale `frame` so its height fits within `max_h`, preserving aspect
/// ratio. Returns the frame unchanged when it already fits.
#[must_use]
pub fn downscale_to_max_height(frame: &Frame, max_h: u32) -> Frame {
    let (w, h) = fit_dimensions(frame.width, frame.height, max_h);
    if w == frame.width && h == frame.height {
        return frame.clone();
    }
    scale_bgra_nearest(frame, w, h)
}

impl Frame {
    /// Number of pixels in the frame.
    #[must_use]
    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    /// The expected BGRA buffer length for the frame's dimensions.
    #[must_use]
    pub fn expected_len(&self) -> usize {
        self.pixel_count() * 4
    }

    /// Whether the buffer length matches the declared dimensions.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.data.len() == self.expected_len()
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
            timestamp_us: 7,
            data,
        }
    }

    #[test]
    fn make_even_floors_and_has_min() {
        assert_eq!(make_even(1920), 1920);
        assert_eq!(make_even(1921), 1920);
        assert_eq!(make_even(1), 2);
        assert_eq!(make_even(0), 2);
    }

    #[test]
    fn fit_dimensions_bounds_height_and_keeps_aspect() {
        // 4K -> 1080 cap: width scales proportionally (3840x2160 -> 1920x1080).
        assert_eq!(fit_dimensions(3840, 2160, 1080), (1920, 1080));
        // Already within the cap: unchanged (made even).
        assert_eq!(fit_dimensions(1280, 720, 1080), (1280, 720));
        // Odd source dimensions get evened.
        assert_eq!(fit_dimensions(1281, 721, 1080), (1280, 720));
    }

    #[test]
    fn scale_preserves_pixel_and_validity() {
        let src = solid(4, 4, [10, 20, 30, 255]);
        let dst = scale_bgra_nearest(&src, 2, 2);
        assert_eq!(dst.width, 2);
        assert_eq!(dst.height, 2);
        assert!(dst.is_valid());
        assert_eq!(&dst.data[0..4], &[10, 20, 30, 255]);
        // Timestamp is carried through unchanged.
        assert_eq!(dst.timestamp_us, 7);
    }

    #[test]
    fn scale_noop_when_dimensions_match() {
        let src = solid(3, 3, [1, 2, 3, 4]);
        let dst = scale_bgra_nearest(&src, 3, 3);
        assert_eq!(dst.data, src.data);
    }

    #[test]
    fn downscale_only_when_exceeding_max_height() {
        let big = solid(200, 200, [9, 9, 9, 255]);
        let small = downscale_to_max_height(&big, 100);
        assert_eq!((small.width, small.height), (100, 100));
        assert!(small.is_valid());

        let already = solid(50, 40, [1, 1, 1, 255]);
        let same = downscale_to_max_height(&already, 100);
        assert_eq!((same.width, same.height), (50, 40));
    }

    #[test]
    fn frame_validity_checks() {
        let f = solid(2, 2, [0, 0, 0, 0]);
        assert!(f.is_valid());
        assert_eq!(f.pixel_count(), 4);
        assert_eq!(f.expected_len(), 16);
    }
}
