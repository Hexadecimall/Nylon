//! Signal processing primitives shared by every device.
//!
//! Each type here is fixed-size and allocation-free: storage that must
//! outlive a call (delay lines, for instance) is borrowed from the caller,
//! which allocates it on the control thread before playback. Every
//! `process` method is safe to call from the audio callback: no locks, no
//! syscalls, no allocation, and no unbounded loops.
//!
//! Sample values are `f32`; coefficients and accumulators that need the
//! headroom use `f64` internally.

pub mod biquad;
pub mod compressor;
pub mod db;
pub mod delay;
pub mod env;
pub mod meter;
pub mod osc;
pub mod pan;
pub mod smooth;

/// Smallest level treated as silence, about -300 dB. Values below this are
/// flushed to zero so denormals never reach a multiply.
pub const SILENCE: f32 = 1.0e-15;

/// Flushes denormal and near-silent values to zero.
#[inline]
#[must_use]
pub fn flush_denormal(value: f32) -> f32 {
    if value.abs() < SILENCE { 0.0 } else { value }
}

/// Clamps to the closed interval, mapping NaN to `low`.
#[inline]
#[must_use]
pub fn clamp(value: f32, low: f32, high: f32) -> f32 {
    // `f32::clamp` panics on a NaN bound and propagates a NaN value; a
    // parameter arriving as NaN should read as the low end instead.
    if value.is_nan() || value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}
