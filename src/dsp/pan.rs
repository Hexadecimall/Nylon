//! Stereo panning laws.
//!
//! Position runs from -1 (hard left) through 0 (center) to +1 (hard
//! right).

use core::f32::consts::FRAC_PI_4;

/// Left and right gains for one position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gains {
    /// Gain applied to the left output.
    pub left: f32,
    /// Gain applied to the right output.
    pub right: f32,
}

/// Constant-power pan: the sum of the squared gains is one at every
/// position, so perceived loudness holds while panning. Center sits at
/// -3 dB on each side.
#[inline]
#[must_use]
pub fn constant_power(position: f32) -> Gains {
    let position = super::clamp(position, -1.0, 1.0);
    // Map -1..1 onto 0..pi/2 and take the two halves of the quadrature pair.
    let angle = (position + 1.0) * FRAC_PI_4;
    Gains {
        left: angle.cos().max(0.0),
        right: angle.sin().max(0.0),
    }
}

/// Linear pan: the sum of the gains is one at every position. Useful when
/// the two channels are later summed to mono, where constant power sums
/// louder in the middle.
#[inline]
#[must_use]
pub fn linear(position: f32) -> Gains {
    let position = super::clamp(position, -1.0, 1.0);
    Gains {
        left: (1.0 - position) * 0.5,
        right: (1.0 + position) * 0.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1e-5
    }

    #[test]
    fn constant_power_holds_energy() {
        for step in -20..=20 {
            let position = step as f32 / 20.0;
            let g = constant_power(position);
            let power = g.left * g.left + g.right * g.right;
            assert!(close(power, 1.0), "{position}: {power}");
            assert!(g.left >= 0.0 && g.right >= 0.0);
        }
    }

    #[test]
    fn constant_power_endpoints_and_center() {
        let left = constant_power(-1.0);
        assert!(close(left.left, 1.0) && close(left.right, 0.0));
        let right = constant_power(1.0);
        assert!(close(right.left, 0.0) && close(right.right, 1.0));
        let center = constant_power(0.0);
        assert!(close(center.left, center.right));
        // Center is -3 dB, not -6 dB.
        assert!(close(center.left, core::f32::consts::FRAC_1_SQRT_2));
    }

    #[test]
    fn linear_sums_to_unity() {
        for step in -10..=10 {
            let position = step as f32 / 10.0;
            let g = linear(position);
            assert!(close(g.left + g.right, 1.0));
        }
        assert!(close(linear(0.0).left, 0.5));
    }

    #[test]
    fn positions_are_clamped_and_monotonic() {
        assert_eq!(constant_power(-5.0), constant_power(-1.0));
        assert_eq!(constant_power(5.0), constant_power(1.0));
        assert_eq!(linear(-5.0), linear(-1.0));
        let mut previous = -1.0;
        for step in -10..=10 {
            let g = constant_power(step as f32 / 10.0);
            assert!(g.right > previous);
            previous = g.right;
        }
    }
}
