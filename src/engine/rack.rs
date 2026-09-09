//! Ordered native and isolated-plugin processing for one graph node.

// off the audio thread
use super::device::{DeviceChain, DeviceConfig, DeviceError, MAX_DEVICES};
use crate::plugin::bridge::Bridge;
use crate::plugin::clap::{NoteEvent, ParameterEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RackError {
    Capacity,
    BufferSize,
    Device(DeviceError),
    StorageCapacity,
}

impl From<DeviceError> for RackError {
    fn from(error: DeviceError) -> Self {
        Self::Device(error)
    }
}

struct PluginStage {
    bridge: Bridge,
    parameter_events: Vec<ParameterEvent>,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    output_left: Vec<f32>,
    output_right: Vec<f32>,
}

impl PluginStage {
    fn new(
        bridge: Bridge,
        max_frames: usize,
        parameter_events: &[ParameterEvent],
    ) -> Result<Self, RackError> {
        if max_frames == 0 || bridge.block_frames() != max_frames {
            return Err(RackError::BufferSize);
        }
        if parameter_events.len() > crate::plugin::clap::MAX_PARAMETER_EVENTS
            || parameter_events
                .iter()
                .any(|event| event.sample_offset != 0 || !event.value.is_finite())
        {
            return Err(RackError::BufferSize);
        }
        Ok(Self {
            bridge,
            parameter_events: parameter_events.to_vec(),
            input_left: zeroed(max_frames)?,
            input_right: zeroed(max_frames)?,
            output_left: zeroed(max_frames)?,
            output_right: zeroed(max_frames)?,
        })
    }
}

enum Stage {
    Native(DeviceChain),
    Plugin(Box<PluginStage>),
}

/// A fixed-capacity device rack prepared on the control thread.
pub struct DeviceRack {
    stages: Vec<Stage>,
    device_count: usize,
    max_frames: usize,
    first: Vec<[f32; 2]>,
    second: Vec<[f32; 2]>,
    delay_storage_frames: usize,
    latency_frames: u32,
}

impl DeviceRack {
    pub fn new(max_frames: usize) -> Result<Self, RackError> {
        if max_frames == 0 {
            return Err(RackError::BufferSize);
        }
        Ok(Self {
            stages: Vec::with_capacity(MAX_DEVICES),
            device_count: 0,
            max_frames,
            first: zeroed_stereo(max_frames)?,
            second: zeroed_stereo(max_frames)?,
            delay_storage_frames: 0,
            latency_frames: 0,
        })
    }

    pub fn native(
        configs: &[DeviceConfig],
        sample_rate: f32,
        max_frames: usize,
    ) -> Result<Self, RackError> {
        let mut rack = Self::new(max_frames)?;
        rack.push_native(configs, sample_rate)?;
        Ok(rack)
    }

    pub fn push_native(
        &mut self,
        configs: &[DeviceConfig],
        sample_rate: f32,
    ) -> Result<(), RackError> {
        if configs.is_empty() {
            return Ok(());
        }
        let count = self
            .device_count
            .checked_add(configs.len())
            .filter(|count| *count <= MAX_DEVICES)
            .ok_or(RackError::Capacity)?;
        let added_latency = configs.iter().try_fold(0_u32, |total, config| {
            total
                .checked_add(
                    config
                        .latency_frames(sample_rate)
                        .map_err(RackError::Device)?,
                )
                .ok_or(RackError::StorageCapacity)
        })?;
        let latency_frames = self
            .latency_frames
            .checked_add(added_latency)
            .ok_or(RackError::StorageCapacity)?;
        let chain = DeviceChain::new(configs, sample_rate)?;
        self.delay_storage_frames = self
            .delay_storage_frames
            .checked_add(chain.delay_storage_frames())
            .ok_or(RackError::StorageCapacity)?;
        self.stages.push(Stage::Native(chain));
        self.device_count = count;
        self.latency_frames = latency_frames;
        Ok(())
    }

    pub fn push_plugin(&mut self, bridge: Bridge) -> Result<(), RackError> {
        self.push_plugin_with_parameters(bridge, &[])
    }

    pub fn push_plugin_with_parameters(
        &mut self,
        bridge: Bridge,
        parameter_events: &[ParameterEvent],
    ) -> Result<(), RackError> {
        if self.device_count == MAX_DEVICES {
            return Err(RackError::Capacity);
        }
        let latency_frames = self
            .latency_frames
            .checked_add(bridge.latency_frames())
            .ok_or(RackError::StorageCapacity)?;
        let stage = PluginStage::new(bridge, self.max_frames, parameter_events)?;
        self.stages.push(Stage::Plugin(Box::new(stage)));
        self.device_count += 1;
        self.latency_frames = latency_frames;
        Ok(())
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.device_count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.device_count == 0
    }

    #[must_use]
    pub const fn delay_storage_frames(&self) -> usize {
        self.delay_storage_frames
    }

    #[must_use]
    pub const fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    // back on the audio thread
    pub fn process(
        &mut self,
        input: &[[f32; 2]],
        sidechain: &[[f32; 2]],
        output: &mut [[f32; 2]],
    ) -> Result<(), RackError> {
        self.process_events(input, sidechain, output, &[])
    }

    pub fn process_events(
        &mut self,
        input: &[[f32; 2]],
        sidechain: &[[f32; 2]],
        output: &mut [[f32; 2]],
        note_events: &[NoteEvent],
    ) -> Result<(), RackError> {
        let frames = input.len();
        if frames > self.max_frames
            || frames != output.len()
            || (!sidechain.is_empty() && sidechain.len() != frames)
        {
            return Err(RackError::BufferSize);
        }
        self.first[..frames].copy_from_slice(input);
        for frame in &mut self.first[..frames] {
            frame[0] = finite_sample(frame[0]);
            frame[1] = finite_sample(frame[1]);
        }
        let mut first_is_source = true;
        for stage in &mut self.stages {
            let result = if first_is_source {
                process_stage(
                    stage,
                    &self.first[..frames],
                    sidechain,
                    &mut self.second[..frames],
                    self.max_frames,
                    note_events,
                )
            } else {
                process_stage(
                    stage,
                    &self.second[..frames],
                    sidechain,
                    &mut self.first[..frames],
                    self.max_frames,
                    note_events,
                )
            };
            result?;
            first_is_source = !first_is_source;
        }
        let source = if first_is_source {
            &self.first[..frames]
        } else {
            &self.second[..frames]
        };
        output.copy_from_slice(source);
        Ok(())
    }
}

fn process_stage(
    stage: &mut Stage,
    input: &[[f32; 2]],
    sidechain: &[[f32; 2]],
    output: &mut [[f32; 2]],
    max_frames: usize,
    note_events: &[NoteEvent],
) -> Result<(), RackError> {
    match stage {
        Stage::Native(chain) => chain.process(input, sidechain, output).map_err(Into::into),
        Stage::Plugin(stage) => {
            for (index, frame) in input.iter().enumerate() {
                stage.input_left[index] = frame[0];
                stage.input_right[index] = frame[1];
            }
            stage.input_left[input.len()..max_frames].fill(0.0);
            stage.input_right[input.len()..max_frames].fill(0.0);
            let note_events = if stage.bridge.input_note_ports() == 0 {
                &[]
            } else {
                note_events
            };
            stage
                .bridge
                .process_stereo(
                    Some((&stage.input_left, &stage.input_right)),
                    &mut stage.output_left,
                    &mut stage.output_right,
                    &stage.parameter_events,
                    note_events,
                )
                .map_err(|_| RackError::BufferSize)?;
            for (index, frame) in output.iter_mut().enumerate() {
                *frame = [stage.output_left[index], stage.output_right[index]];
            }
            Ok(())
        }
    }
}

// off the audio thread
fn zeroed(frames: usize) -> Result<Vec<f32>, RackError> {
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(frames)
        .map_err(|_| RackError::StorageCapacity)?;
    storage.resize(frames, 0.0);
    Ok(storage)
}

fn zeroed_stereo(frames: usize) -> Result<Vec<[f32; 2]>, RackError> {
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(frames)
        .map_err(|_| RackError::StorageCapacity)?;
    storage.resize(frames, [0.0; 2]);
    Ok(storage)
}
// back on the audio thread

#[inline]
fn finite_sample(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::bridge::BlockProcessor;
    use crate::plugin::clap::{NoteEvent, ParameterEvent};

    struct Gain(f32);

    struct ParameterGain(u32);

    struct NoteSignal;

    impl BlockProcessor for Gain {
        fn process_block(
            &mut self,
            input: Option<(&[f32], &[f32])>,
            output_left: &mut [f32],
            output_right: &mut [f32],
            _: &[ParameterEvent],
            _: &[NoteEvent],
        ) -> bool {
            let (left, right) = input.unwrap();
            for index in 0..left.len() {
                output_left[index] = left[index] * self.0;
                output_right[index] = right[index] * self.0;
            }
            true
        }
    }

    impl BlockProcessor for ParameterGain {
        fn process_block(
            &mut self,
            input: Option<(&[f32], &[f32])>,
            output_left: &mut [f32],
            output_right: &mut [f32],
            parameter_events: &[ParameterEvent],
            _: &[NoteEvent],
        ) -> bool {
            let gain = match parameter_events {
                [event] if event.identifier == self.0 => event.value as f32,
                _ => 0.0,
            };
            let (left, right) = input.unwrap();
            for index in 0..left.len() {
                output_left[index] = left[index] * gain;
                output_right[index] = right[index] * gain;
            }
            true
        }
    }

    impl BlockProcessor for NoteSignal {
        fn process_block(
            &mut self,
            _: Option<(&[f32], &[f32])>,
            output_left: &mut [f32],
            output_right: &mut [f32],
            _: &[ParameterEvent],
            note_events: &[NoteEvent],
        ) -> bool {
            output_left.fill(0.0);
            output_right.fill(0.0);
            for event in note_events {
                let index = event.sample_offset as usize;
                output_left[index] = event.velocity as f32;
                output_right[index] = event.key as f32 / 127.0;
            }
            true
        }
    }

    fn utility(gain_db: f32) -> DeviceConfig {
        DeviceConfig {
            enabled: true,
            kind: super::super::device::DeviceKind::Utility {
                gain_db,
                width: 1.0,
                balance: 0.0,
            },
        }
    }

    #[test]
    fn native_and_plugin_stages_keep_their_order() {
        let bridge = Bridge::new(Gain(3.0), 4, 2, 0).unwrap();
        let mut rack = DeviceRack::new(4).unwrap();
        rack.push_native(&[utility(-6.020_6)], 48_000.0).unwrap();
        rack.push_plugin(bridge).unwrap();
        rack.push_native(&[utility(-6.020_6)], 48_000.0).unwrap();
        assert_eq!(rack.len(), 3);
        assert_eq!(rack.latency_frames(), 4);

        let input = [[0.4, -0.2]; 4];
        let mut output = [[0.0; 2]; 4];
        for _ in 0..10_000 {
            rack.process(&input, &[], &mut output).unwrap();
            if (output[0][0] - 0.3).abs() < 1e-4 {
                break;
            }
            std::thread::yield_now();
        }
        for frame in output {
            assert!((frame[0] - 0.3).abs() < 1e-4, "{frame:?}");
            assert!((frame[1] + 0.15).abs() < 1e-4, "{frame:?}");
        }
    }

    #[test]
    fn short_blocks_are_zero_padded_for_the_plugin_worker() {
        let bridge = Bridge::new(Gain(2.0), 8, 2, 0).unwrap();
        let mut rack = DeviceRack::new(8).unwrap();
        rack.push_plugin(bridge).unwrap();
        let input = [[0.25, -0.5]; 3];
        let mut output = [[0.0; 2]; 3];
        for _ in 0..10_000 {
            rack.process(&input, &[], &mut output).unwrap();
            if output == [[0.5, -1.0]; 3] {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(output, [[0.5, -1.0]; 3]);
    }

    #[test]
    fn plugin_note_events_retain_their_sample_offset() {
        let bridge = Bridge::new(NoteSignal, 8, 2, 0).unwrap();
        let mut rack = DeviceRack::new(8).unwrap();
        rack.push_plugin(bridge).unwrap();
        let input = [[0.0; 2]; 8];
        let event = NoteEvent {
            sample_offset: 5,
            kind: 0,
            note_id: -1,
            port_index: 0,
            channel: 0,
            key: 64,
            velocity: 0.75,
        };
        let mut output = [[0.0; 2]; 8];
        for _ in 0..10_000 {
            rack.process_events(&input, &[], &mut output, &[event])
                .unwrap();
            if output[5][0] == 0.75 {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(output[4], [0.0, 0.0]);
        assert_eq!(output[5], [0.75, 64.0 / 127.0]);
        assert_eq!(output[6], [0.0, 0.0]);
    }

    #[test]
    fn plugin_wildcard_choke_releases_all_notes() {
        let bridge = Bridge::new(Gain(1.0), 4, 2, 0).unwrap();
        let mut rack = DeviceRack::new(4).unwrap();
        rack.push_plugin(bridge).unwrap();
        let input = [[0.25, -0.25]; 4];
        let mut output = [[0.0; 2]; 4];
        let choke = NoteEvent {
            sample_offset: 0,
            kind: 2,
            note_id: -1,
            port_index: 0,
            channel: -1,
            key: -1,
            velocity: 0.0,
        };
        for _ in 0..10_000 {
            rack.process_events(&input, &[], &mut output, &[choke])
                .unwrap();
            if output == input {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(output, input);
    }

    #[test]
    fn plugin_parameter_values_are_delivered_only_to_their_stage() {
        let mut rack = DeviceRack::new(4).unwrap();
        rack.push_plugin_with_parameters(
            Bridge::new(ParameterGain(11), 4, 2, 0).unwrap(),
            &[ParameterEvent {
                sample_offset: 0,
                identifier: 11,
                value: 2.0,
            }],
        )
        .unwrap();
        rack.push_plugin_with_parameters(
            Bridge::new(ParameterGain(29), 4, 2, 0).unwrap(),
            &[ParameterEvent {
                sample_offset: 0,
                identifier: 29,
                value: 3.0,
            }],
        )
        .unwrap();
        let input = [[1.0, -1.0]; 4];
        let mut output = [[0.0; 2]; 4];
        for _ in 0..10_000 {
            rack.process(&input, &[], &mut output).unwrap();
            if output == [[6.0, -6.0]; 4] {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(output, [[6.0, -6.0]; 4]);
    }
}
