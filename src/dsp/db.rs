//! Decibel conversions.
//!
//! Amplitude ratios throughout use 20 log10, the convention for a gain
//! fader. Silence is negative infinity rather than a large negative number
//! so a muted signal compares equal regardless of how it was produced.

/// Level at or below which [`to_linear`] returns exactly zero.
pub const MINUS_INFINITY: f32 = -300.0;

/// Converts decibels to a linear amplitude ratio.
#[inline]
#[must_use]
pub fn to_linear(decibels: f32) -> f32 {
    if decibels <= MINUS_INFINITY || decibels.is_nan() {
        0.0
    } else {
        10.0_f32.powf(decibels * 0.05)
    }
}

/// Converts a linear amplitude ratio to decibels. Zero and negative
/// amplitudes are negative infinity.
#[inline]
#[must_use]
pub fn from_linear(amplitude: f32) -> f32 {
    let magnitude = amplitude.abs();
    if magnitude <= 0.0 || magnitude.is_nan() {
        f32::NEG_INFINITY
    } else {
        20.0 * magnitude.log10()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tolerance: f32) -> bool {
        (a - b).abs() <= tolerance
    }

    #[test]
    fn unity_and_common_ratios() {
        assert!(close(to_linear(0.0), 1.0, 1e-6));
        assert!(close(to_linear(6.020_6), 2.0, 1e-4));
        assert!(close(to_linear(-6.020_6), 0.5, 1e-4));
        assert!(close(to_linear(20.0), 10.0, 1e-4));
        assert!(close(to_linear(-20.0), 0.1, 1e-6));
    }

    #[test]
    fn round_trips() {
        for level in [-96.0, -48.0, -12.0, -0.5, 0.0, 3.0, 6.0] {
            assert!(close(from_linear(to_linear(level)), level, 1e-3), "{level}");
        }
        for amplitude in [1e-4, 0.01, 0.5, 1.0, 2.0] {
            assert!(close(
                to_linear(from_linear(amplitude)),
                amplitude,
                amplitude * 1e-4
            ));
        }
    }

    #[test]
    fn silence_maps_both_ways() {
        assert_eq!(to_linear(MINUS_INFINITY), 0.0);
        assert_eq!(to_linear(f32::NEG_INFINITY), 0.0);
        assert_eq!(to_linear(-1000.0), 0.0);
        assert_eq!(from_linear(0.0), f32::NEG_INFINITY);
        assert_eq!(from_linear(-0.0), f32::NEG_INFINITY);
    }

    #[test]
    fn sign_is_ignored_and_nan_is_silent() {
        assert_eq!(from_linear(-0.5), from_linear(0.5));
        assert_eq!(to_linear(f32::NAN), 0.0);
        assert_eq!(from_linear(f32::NAN), f32::NEG_INFINITY);
    }
}
