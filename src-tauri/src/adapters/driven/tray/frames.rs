//! Pre-rendered RGBA frames for the tray pulse animation.
//!
//! Stays in code rather than shipping eight binary PNG assets — the design
//! ("petit dot orange qui pulse") is small enough that a procedural generator
//! is easier to tweak, has no asset-checksum churn, and lets us cover the
//! shape with regular unit tests.

const FRAME_SIZE: u32 = 32;
const FRAME_COUNT: usize = 8;

const DOT_CX: i32 = 16;
const DOT_CY: i32 = 16;
const MIN_RADIUS: f32 = 3.0;
const MAX_RADIUS: f32 = 7.0;

const ORANGE_R: u8 = 0xF5;
const ORANGE_G: u8 = 0x9E;
const ORANGE_B: u8 = 0x0B;

/// One frame of the tray animation: row-major RGBA buffer.
#[derive(Debug, Clone)]
pub struct TrayFrame {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Generates the `FRAME_COUNT` pulse frames used by the animator.
///
/// The dot grows from `MIN_RADIUS` to `MAX_RADIUS` and back over the cycle,
/// so the animation reads as a slow pulse rather than a rotating spinner.
pub fn pulse_frames() -> Vec<TrayFrame> {
    (0..FRAME_COUNT).map(render_frame).collect()
}

fn render_frame(index: usize) -> TrayFrame {
    let phase = index as f32 / FRAME_COUNT as f32;
    let radius = pulse_radius(phase);
    let mut rgba = vec![0u8; (FRAME_SIZE * FRAME_SIZE * 4) as usize];
    for y in 0..FRAME_SIZE as i32 {
        for x in 0..FRAME_SIZE as i32 {
            let dx = (x - DOT_CX) as f32;
            let dy = (y - DOT_CY) as f32;
            let dist = (dx * dx + dy * dy).sqrt();
            let alpha = pixel_alpha(dist, radius);
            if alpha == 0 {
                continue;
            }
            let offset = ((y as u32 * FRAME_SIZE + x as u32) * 4) as usize;
            rgba[offset] = ORANGE_R;
            rgba[offset + 1] = ORANGE_G;
            rgba[offset + 2] = ORANGE_B;
            rgba[offset + 3] = alpha;
        }
    }
    TrayFrame {
        rgba,
        width: FRAME_SIZE,
        height: FRAME_SIZE,
    }
}

/// Triangular wave: phase 0→0.5 grows the dot, 0.5→1 shrinks it back.
fn pulse_radius(phase: f32) -> f32 {
    let triangular = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    MIN_RADIUS + (MAX_RADIUS - MIN_RADIUS) * triangular
}

fn pixel_alpha(dist: f32, radius: f32) -> u8 {
    if dist <= radius - 1.0 {
        255
    } else if dist >= radius {
        0
    } else {
        let t = radius - dist;
        (t.clamp(0.0, 1.0) * 255.0) as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pulse_frames_returns_eight_frames() {
        assert_eq!(pulse_frames().len(), FRAME_COUNT);
    }

    #[test]
    fn test_each_frame_is_32x32_rgba() {
        for frame in pulse_frames() {
            assert_eq!(frame.width, FRAME_SIZE);
            assert_eq!(frame.height, FRAME_SIZE);
            assert_eq!(frame.rgba.len(), (FRAME_SIZE * FRAME_SIZE * 4) as usize);
        }
    }

    #[test]
    fn test_center_pixel_is_orange_opaque_in_every_frame() {
        for (i, frame) in pulse_frames().iter().enumerate() {
            let center = ((DOT_CY as u32 * FRAME_SIZE + DOT_CX as u32) * 4) as usize;
            assert_eq!(frame.rgba[center], ORANGE_R, "frame {i} R");
            assert_eq!(frame.rgba[center + 1], ORANGE_G, "frame {i} G");
            assert_eq!(frame.rgba[center + 2], ORANGE_B, "frame {i} B");
            assert_eq!(frame.rgba[center + 3], 255, "frame {i} alpha");
        }
    }

    #[test]
    fn test_corner_pixel_is_transparent() {
        for frame in pulse_frames() {
            assert_eq!(frame.rgba[3], 0, "top-left alpha should be 0");
            let last = frame.rgba.len() - 1;
            assert_eq!(frame.rgba[last], 0, "bottom-right alpha should be 0");
        }
    }

    #[test]
    fn test_frames_differ_across_cycle() {
        let frames = pulse_frames();
        // Smallest (frame 0) and largest (frame at peak) must differ.
        let first = &frames[0];
        let peak = &frames[FRAME_COUNT / 2];
        assert_ne!(first.rgba, peak.rgba);
    }

    #[test]
    fn test_pulse_radius_is_min_at_phase_zero() {
        assert!((pulse_radius(0.0) - MIN_RADIUS).abs() < 1e-4);
    }

    #[test]
    fn test_pulse_radius_is_max_at_phase_half() {
        assert!((pulse_radius(0.5) - MAX_RADIUS).abs() < 1e-4);
    }
}
