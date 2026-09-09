//! Native audio device chains for routed tracks and buses.

use crate::dsp::biquad::{Biquad, Coefficients, Kind as FilterKind};
use crate::dsp::compressor::{Compressor, Parameters as CompressorParameters};
use crate::dsp::db;
use crate::dsp::delay::DelayLine;

pub const MAX_DEVICES: usize = 16;
pub const MAX_DELAY_STORAGE_FRAMES: usize = 3_840_004;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceConfig {
    pub enabled: bool,
    pub kind: DeviceKind,
}

impl DeviceConfig {
    /// Checks configuration without allocating processor storage.
    pub fn validate(self, sample_rate: f32) -> Result<(), DeviceError> {
        validate_sample_rate(sample_rate)?;
        validate_kind(self.kind, sample_rate).map(|_| ())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeviceKind {
    Utility {
        gain_db: f32,
        width: f32,
        balance: f32,
    },
    Equalizer {
        kind: FilterKind,
        frequency: f32,
        q: f32,
        gain_db: f32,
    },
    Compressor {
        parameters: CompressorParameters,
        external_sidechain: bool,
    },
    StereoDelay {
        delay_seconds: f32,
        feedback: f32,
        mix: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    Capacity,
    InvalidSampleRate,
    InvalidParameter,
    BufferSize,
    StorageCapacity,
}

enum Processor {
    Utility {
        gain: f32,
        width: f32,
        balance: f32,
    },
    Equalizer {
        left: Biquad,
        right: Biquad,
    },
    Compressor {
        processor: Compressor,
        external_sidechain: bool,
    },
    StereoDelay {
        left: DelayLine,
        right: DelayLine,
        left_storage: Vec<f32>,
        right_storage: Vec<f32>,
        delay_frames: f32,
        feedback: f32,
        mix: f32,
    },
}

struct Device {
    enabled: bool,
    processor: Processor,
}

pub struct DeviceChain {
    devices: Vec<Device>,
    delay_storage_frames: usize,
}

impl DeviceChain {
    /// Builds a chain and all storage before it reaches the callback.
    pub fn new(configs: &[DeviceConfig], sample_rate: f32) -> Result<Self, DeviceError> {
        if configs.len() > MAX_DEVICES {
            return Err(DeviceError::Capacity);
        }
        validate_sample_rate(sample_rate)?;
        let mut devices = Vec::new();
        devices
            .try_reserve_exact(configs.len())
            .map_err(|_| DeviceError::StorageCapacity)?;
        let mut delay_storage = 0_usize;
        for config in configs {
            let delay_frames = validate_kind(config.kind, sample_rate)?;
            let processor = match config.kind {
                DeviceKind::Utility {
                    gain_db,
                    width,
                    balance,
                } => Processor::Utility {
                    gain: db::to_linear(gain_db),
                    width,
                    balance,
                },
                DeviceKind::Equalizer {
                    kind,
                    frequency,
                    q,
                    gain_db,
                } => {
                    let coefficients =
                        Coefficients::design(kind, frequency, q, gain_db, sample_rate);
                    Processor::Equalizer {
                        left: Biquad::new(coefficients),
                        right: Biquad::new(coefficients),
                    }
                }
                DeviceKind::Compressor {
                    parameters,
                    external_sidechain,
                } => Processor::Compressor {
                    processor: Compressor::new(sample_rate, parameters),
                    external_sidechain,
                },
                DeviceKind::StereoDelay {
                    delay_seconds,
                    feedback,
                    mix,
                } => {
                    let frames = delay_frames;
                    delay_storage = delay_storage
                        .checked_add(frames.saturating_mul(2))
                        .filter(|value| *value <= MAX_DELAY_STORAGE_FRAMES)
                        .ok_or(DeviceError::StorageCapacity)?;
                    let left_storage = zeroed(frames)?;
                    let right_storage = zeroed(frames)?;
                    Processor::StereoDelay {
                        left: DelayLine::new(),
                        right: DelayLine::new(),
                        left_storage,
                        right_storage,
                        delay_frames: delay_seconds * sample_rate,
                        feedback,
                        mix,
                    }
                }
            };
            devices.push(Device {
                enabled: config.enabled,
                processor,
            });
        }
        Ok(Self {
            devices,
            delay_storage_frames: delay_storage,
        })
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.devices.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    #[must_use]
    pub const fn delay_storage_frames(&self) -> usize {
        self.delay_storage_frames
    }

    /// Runs the chain in place. Sidechain samples are read only by devices
    /// configured for an external detector.
    pub fn process(
        &mut self,
        input: &[[f32; 2]],
        sidechain: &[[f32; 2]],
        output: &mut [[f32; 2]],
    ) -> Result<(), DeviceError> {
        if input.len() != output.len() || (!sidechain.is_empty() && sidechain.len() != input.len())
        {
            return Err(DeviceError::BufferSize);
        }
        output.copy_from_slice(input);
        for frame in &mut *output {
            frame[0] = finite_sample(frame[0]);
            frame[1] = finite_sample(frame[1]);
        }
        for device in &mut self.devices {
            if !device.enabled {
                continue;
            }
            device.processor.process(output, sidechain);
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        for device in &mut self.devices {
            device.processor.reset();
        }
    }
}

fn validate_sample_rate(sample_rate: f32) -> Result<(), DeviceError> {
    if sample_rate.is_finite() && (8_000.0..=192_000.0).contains(&sample_rate) {
        Ok(())
    } else {
        Err(DeviceError::InvalidSampleRate)
    }
}

fn validate_kind(kind: DeviceKind, sample_rate: f32) -> Result<usize, DeviceError> {
    match kind {
        DeviceKind::Utility {
            gain_db,
            width,
            balance,
        } if finite_range(gain_db, -120.0, 24.0)
            && finite_range(width, 0.0, 2.0)
            && finite_range(balance, -1.0, 1.0) =>
        {
            Ok(0)
        }
        DeviceKind::Equalizer {
            frequency,
            q,
            gain_db,
            ..
        } if finite_range(frequency, 1.0, sample_rate * 0.4975)
            && finite_range(q, 0.001, 100.0)
            && finite_range(gain_db, -96.0, 96.0) =>
        {
            Ok(0)
        }
        DeviceKind::Compressor { parameters, .. } if compressor_parameters_valid(parameters) => {
            Ok(0)
        }
        DeviceKind::StereoDelay {
            delay_seconds,
            feedback,
            mix,
        } if finite_range(delay_seconds, 0.0, 10.0)
            && finite_range(feedback, -0.99, 0.99)
            && finite_range(mix, 0.0, 1.0) =>
        {
            Ok((delay_seconds * sample_rate).ceil() as usize + 2)
        }
        _ => Err(DeviceError::InvalidParameter),
    }
}

impl Processor {
    fn process(&mut self, audio: &mut [[f32; 2]], sidechain: &[[f32; 2]]) {
        match self {
            Self::Utility {
                gain,
                width,
                balance,
            } => {
                let left_balance = 1.0 - balance.max(0.0);
                let right_balance = 1.0 + balance.min(0.0);
                for frame in audio {
                    let mid = (frame[0] + frame[1]) * 0.5;
                    let side = (frame[0] - frame[1]) * 0.5 * *width;
                    frame[0] = (mid + side) * *gain * left_balance;
                    frame[1] = (mid - side) * *gain * right_balance;
                }
            }
            Self::Equalizer { left, right } => {
                for frame in audio {
                    frame[0] = left.process(frame[0]);
                    frame[1] = right.process(frame[1]);
                }
            }
            Self::Compressor {
                processor,
                external_sidechain,
            } => {
                for (index, frame) in audio.iter_mut().enumerate() {
                    let detector = if *external_sidechain {
                        sidechain.get(index).copied().unwrap_or([0.0; 2])
                    } else {
                        *frame
                    };
                    (frame[0], frame[1]) = processor.process_stereo_sidechain(
                        frame[0],
                        frame[1],
                        detector[0],
                        detector[1],
                    );
                }
            }
            Self::StereoDelay {
                left,
                right,
                left_storage,
                right_storage,
                delay_frames,
                feedback,
                mix,
            } => {
                for frame in audio {
                    let wet_left = left.read_interpolated(left_storage, *delay_frames);
                    let wet_right = right.read_interpolated(right_storage, *delay_frames);
                    left.write(left_storage, frame[0] + wet_left * *feedback);
                    right.write(right_storage, frame[1] + wet_right * *feedback);
                    frame[0] += (wet_left - frame[0]) * *mix;
                    frame[1] += (wet_right - frame[1]) * *mix;
                }
            }
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Equalizer { left, right } => {
                left.reset();
                right.reset();
            }
            Self::Compressor { processor, .. } => processor.reset(),
            Self::StereoDelay {
                left,
                right,
                left_storage,
                right_storage,
                ..
            } => {
                left.reset(left_storage);
                right.reset(right_storage);
            }
            Self::Utility { .. } => {}
        }
    }
}

fn finite_range(value: f32, low: f32, high: f32) -> bool {
    value.is_finite() && (low..=high).contains(&value)
}

fn finite_sample(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn compressor_parameters_valid(parameters: CompressorParameters) -> bool {
    finite_range(parameters.threshold_db, -96.0, 0.0)
        && finite_range(parameters.ratio, 1.0, 100.0)
        && finite_range(parameters.knee_db, 0.0, 48.0)
        && finite_range(parameters.attack_seconds, 0.0, 10.0)
        && finite_range(parameters.release_seconds, 0.0, 30.0)
        && finite_range(parameters.makeup_db, -48.0, 48.0)
}

fn zeroed(frames: usize) -> Result<Vec<f32>, DeviceError> {
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(frames)
        .map_err(|_| DeviceError::StorageCapacity)?;
    storage.resize(frames, 0.0);
    Ok(storage)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn utility(gain_db: f32, width: f32, balance: f32) -> DeviceConfig {
        DeviceConfig {
            enabled: true,
            kind: DeviceKind::Utility {
                gain_db,
                width,
                balance,
            },
        }
    }

    #[test]
    fn utility_controls_gain_width_and_balance() {
        let mut chain = DeviceChain::new(&[utility(-6.020_6, 0.0, 0.5)], RATE).unwrap();
        let input = [[1.0, -1.0], [0.5, 0.5]];
        let mut output = [[0.0; 2]; 2];
        chain.process(&input, &[], &mut output).unwrap();
        assert_eq!(output[0], [0.0, 0.0]);
        assert!((output[1][0] - 0.125).abs() < 1e-5);
        assert!((output[1][1] - 0.25).abs() < 1e-5);
    }

    #[test]
    fn bypassed_devices_leave_audio_unchanged() {
        let mut config = utility(-24.0, 0.0, -1.0);
        config.enabled = false;
        let mut chain = DeviceChain::new(&[config], RATE).unwrap();
        let input = [[0.25, -0.5]; 8];
        let mut output = [[0.0; 2]; 8];
        chain.process(&input, &[], &mut output).unwrap();
        assert_eq!(output, input);
    }

    #[test]
    fn equalizer_processes_both_channels_independently() {
        let config = DeviceConfig {
            enabled: true,
            kind: DeviceKind::Equalizer {
                kind: FilterKind::LowPass,
                frequency: 800.0,
                q: 0.707,
                gain_db: 0.0,
            },
        };
        let mut chain = DeviceChain::new(&[config], RATE).unwrap();
        let mut input = [[0.0; 2]; 128];
        input[0] = [1.0, -0.5];
        let mut output = [[0.0; 2]; 128];
        chain.process(&input, &[], &mut output).unwrap();
        assert!(output[0][0].abs() < 1.0);
        for frame in output {
            assert!((frame[0] + frame[1] * 2.0).abs() < 1e-5);
        }
    }

    #[test]
    fn compressor_can_follow_an_external_sidechain() {
        let config = DeviceConfig {
            enabled: true,
            kind: DeviceKind::Compressor {
                parameters: CompressorParameters {
                    threshold_db: -20.0,
                    ratio: 10.0,
                    knee_db: 0.0,
                    attack_seconds: 0.0,
                    release_seconds: 0.0,
                    makeup_db: 0.0,
                },
                external_sidechain: true,
            },
        };
        let mut chain = DeviceChain::new(&[config], RATE).unwrap();
        let input = [[0.1, -0.05]; 4];
        let sidechain = [[1.0, 0.5]; 4];
        let mut output = [[0.0; 2]; 4];
        chain.process(&input, &sidechain, &mut output).unwrap();
        assert!(output[0][0] < input[0][0]);
        assert!((output[0][0] / output[0][1] + 2.0).abs() < 1e-5);
    }

    #[test]
    fn stereo_delay_produces_feedback_and_reset_clears_it() {
        let config = DeviceConfig {
            enabled: true,
            kind: DeviceKind::StereoDelay {
                delay_seconds: 4.0 / RATE,
                feedback: 0.5,
                mix: 1.0,
            },
        };
        let mut chain = DeviceChain::new(&[config], RATE).unwrap();
        let mut input = [[0.0; 2]; 16];
        input[0] = [1.0, -1.0];
        let mut output = [[0.0; 2]; 16];
        chain.process(&input, &[], &mut output).unwrap();
        assert_eq!(output[5], [1.0, -1.0]);
        assert_eq!(output[10], [0.5, -0.5]);
        chain.reset();
        let silence = [[0.0; 2]; 16];
        chain.process(&silence, &[], &mut output).unwrap();
        assert_eq!(output, silence);
    }

    #[test]
    fn invalid_configuration_and_buffers_are_rejected() {
        assert!(matches!(
            DeviceChain::new(&[utility(f32::NAN, 1.0, 0.0)], RATE),
            Err(DeviceError::InvalidParameter)
        ));
        assert!(matches!(
            DeviceChain::new(&[utility(0.0, 1.0, 0.0); MAX_DEVICES + 1], RATE),
            Err(DeviceError::Capacity)
        ));
        let mut chain = DeviceChain::new(&[], RATE).unwrap();
        let input = [[0.0; 2]; 2];
        let mut output = [[0.0; 2]; 1];
        assert_eq!(
            chain.process(&input, &[], &mut output),
            Err(DeviceError::BufferSize)
        );

        let mut chain = DeviceChain::new(&[utility(0.0, 1.0, 0.0)], RATE).unwrap();
        let input = [[f32::NAN, f32::INFINITY]];
        chain.process(&input, &[], &mut output).unwrap();
        assert_eq!(output, [[0.0; 2]]);
    }

    #[test]
    fn processing_is_deterministic() {
        let configs = [
            utility(-3.0, 1.25, -0.2),
            DeviceConfig {
                enabled: true,
                kind: DeviceKind::StereoDelay {
                    delay_seconds: 0.001,
                    feedback: 0.3,
                    mix: 0.4,
                },
            },
        ];
        let input: Vec<[f32; 2]> = (0..512)
            .map(|index| [(index as f32 * 0.1).sin(), (index as f32 * 0.07).cos()])
            .collect();
        let render = || {
            let mut chain = DeviceChain::new(&configs, RATE).unwrap();
            let mut output = vec![[0.0; 2]; input.len()];
            chain.process(&input, &[], &mut output).unwrap();
            output
        };
        assert_eq!(render(), render());
    }
}
