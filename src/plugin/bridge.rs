//! Fixed-latency bridge between audio rendering and plugin process I/O.

// off the audio thread
use super::clap::{MAX_NOTE_EVENTS, MAX_PARAMETER_EVENTS, NoteEvent, ParameterEvent};
use super::worker::Client;
use crate::spsc::{Consumer, Producer, SpscQueue};
use std::hint::spin_loop;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};

pub const MAX_QUEUE_DEPTH: usize = 64;
const MAX_QUEUED_FRAMES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfiguration,
    InvalidBlock,
}

pub trait BlockProcessor: Send + 'static {
    fn process_block(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        parameter_events: &[ParameterEvent],
        note_events: &[NoteEvent],
    ) -> bool;
}

impl BlockProcessor for Client {
    fn process_block(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        parameter_events: &[ParameterEvent],
        note_events: &[NoteEvent],
    ) -> bool {
        self.process_stereo(
            input,
            output_left,
            output_right,
            parameter_events,
            note_events,
        )
        .is_ok()
    }
}

#[derive(Default)]
struct Counters {
    submitted: AtomicU64,
    completed: AtomicU64,
    underruns: AtomicU64,
    queue_drops: AtomicU64,
    worker_failures: AtomicU64,
}

struct Block {
    sequence: u64,
    has_input: bool,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    output_left: Vec<f32>,
    output_right: Vec<f32>,
    parameter_events: Vec<ParameterEvent>,
    note_events: Vec<NoteEvent>,
}

impl Block {
    fn new(frames: usize) -> Self {
        Self {
            sequence: 0,
            has_input: false,
            input_left: vec![0.0; frames],
            input_right: vec![0.0; frames],
            output_left: vec![0.0; frames],
            output_right: vec![0.0; frames],
            parameter_events: Vec::with_capacity(MAX_PARAMETER_EVENTS),
            note_events: Vec::with_capacity(MAX_NOTE_EVENTS),
        }
    }
}

/// Adds one configured audio block of latency and never waits for process I/O.
pub struct Bridge {
    requests: Producer<Block>,
    responses: Consumer<Block>,
    pool: Vec<Block>,
    running: Arc<AtomicBool>,
    counters: Arc<Counters>,
    worker: Option<JoinHandle<()>>,
    frames: usize,
    input_note_ports: u32,
    latency_frames: u32,
    sequence: u64,
    fallback_left: Vec<f32>,
    fallback_right: Vec<f32>,
}

impl Bridge {
    pub fn from_client(client: Client, frames: usize, queue_depth: usize) -> Result<Self, Error> {
        let latency = client.latency_frames();
        let input_note_ports = client.input_note_ports();
        Self::new_with_ports(client, frames, queue_depth, latency, input_note_ports)
    }

    pub fn new<P: BlockProcessor>(
        processor: P,
        frames: usize,
        queue_depth: usize,
        processor_latency_frames: u32,
    ) -> Result<Self, Error> {
        Self::new_with_ports(
            processor,
            frames,
            queue_depth,
            processor_latency_frames,
            u32::MAX,
        )
    }

    fn new_with_ports<P: BlockProcessor>(
        processor: P,
        frames: usize,
        queue_depth: usize,
        processor_latency_frames: u32,
        input_note_ports: u32,
    ) -> Result<Self, Error> {
        if frames == 0
            || !(2..=MAX_QUEUE_DEPTH).contains(&queue_depth)
            || frames
                .checked_mul(queue_depth)
                .is_none_or(|total| total > MAX_QUEUED_FRAMES)
        {
            return Err(Error::InvalidConfiguration);
        }
        let latency_frames = processor_latency_frames
            .checked_add(frames as u32)
            .ok_or(Error::InvalidConfiguration)?;
        let (request_tx, request_rx) = SpscQueue::with_capacity(queue_depth);
        let (response_tx, response_rx) = SpscQueue::with_capacity(queue_depth);
        let running = Arc::new(AtomicBool::new(true));
        let counters = Arc::new(Counters::default());
        let worker_running = Arc::clone(&running);
        let worker_counters = Arc::clone(&counters);
        let worker = thread::Builder::new()
            .name("nylon-plugin-ipc".into())
            .spawn(move || {
                worker_loop(
                    processor,
                    request_rx,
                    response_tx,
                    &worker_running,
                    &worker_counters,
                );
            })
            .map_err(|_| Error::InvalidConfiguration)?;
        let mut pool = Vec::with_capacity(queue_depth);
        for _ in 0..queue_depth {
            pool.push(Block::new(frames));
        }
        Ok(Self {
            requests: request_tx,
            responses: response_rx,
            pool,
            running,
            counters,
            worker: Some(worker),
            frames,
            input_note_ports,
            latency_frames,
            sequence: 0,
            fallback_left: vec![0.0; frames],
            fallback_right: vec![0.0; frames],
        })
    }

    #[must_use]
    pub const fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    #[must_use]
    pub const fn block_frames(&self) -> usize {
        self.frames
    }

    #[must_use]
    pub fn submitted_blocks(&self) -> u64 {
        self.counters.submitted.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn completed_blocks(&self) -> u64 {
        self.counters.completed.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn underruns(&self) -> u64 {
        self.counters.underruns.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn queue_drops(&self) -> u64 {
        self.counters.queue_drops.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn worker_failures(&self) -> u64 {
        self.counters.worker_failures.load(Ordering::Relaxed)
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }

    // back on the audio thread
    pub fn process_stereo(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        parameter_events: &[ParameterEvent],
        note_events: &[NoteEvent],
    ) -> Result<(), Error> {
        if output_left.len() != self.frames
            || output_right.len() != self.frames
            || input.is_some_and(|(left, right)| {
                left.len() != self.frames || right.len() != self.frames
            })
            || parameter_events.len() > MAX_PARAMETER_EVENTS
            || note_events.len() > MAX_NOTE_EVENTS
            || parameter_events.iter().any(|event| {
                event.sample_offset as usize >= self.frames || !event.value.is_finite()
            })
            || parameter_events
                .windows(2)
                .any(|events| events[0].sample_offset > events[1].sample_offset)
            || note_events.iter().any(|event| {
                event.sample_offset as usize >= self.frames
                    || event.kind > 2
                    || event.port_index < 0
                    || event.port_index as u32 >= self.input_note_ports
                    || !(0..=15).contains(&event.channel)
                    || !(0..=127).contains(&event.key)
                    || !event.velocity.is_finite()
                    || !(0.0..=1.0).contains(&event.velocity)
            })
            || note_events
                .windows(2)
                .any(|events| events[0].sample_offset > events[1].sample_offset)
        {
            return Err(Error::InvalidBlock);
        }

        output_left.copy_from_slice(&self.fallback_left);
        output_right.copy_from_slice(&self.fallback_right);
        match input {
            Some((left, right)) => {
                self.fallback_left.copy_from_slice(left);
                self.fallback_right.copy_from_slice(right);
            }
            None => {
                self.fallback_left.fill(0.0);
                self.fallback_right.fill(0.0);
            }
        }

        let expected = self.sequence.checked_sub(1);
        let mut received = false;
        while let Some(block) = self.responses.pop() {
            if Some(block.sequence) == expected {
                output_left.copy_from_slice(&block.output_left);
                output_right.copy_from_slice(&block.output_right);
                received = true;
            }
            self.pool.push(block);
        }
        if expected.is_some() && !received {
            self.counters.underruns.fetch_add(1, Ordering::Relaxed);
        }

        if !self.running.load(Ordering::Acquire) {
            self.counters.queue_drops.fetch_add(1, Ordering::Relaxed);
            self.sequence = self.sequence.wrapping_add(1);
            return Ok(());
        }
        let Some(mut block) = self.pool.pop() else {
            self.counters.queue_drops.fetch_add(1, Ordering::Relaxed);
            self.sequence = self.sequence.wrapping_add(1);
            return Ok(());
        };
        block.sequence = self.sequence;
        block.has_input = input.is_some();
        if let Some((left, right)) = input {
            block.input_left.copy_from_slice(left);
            block.input_right.copy_from_slice(right);
        } else {
            block.input_left.fill(0.0);
            block.input_right.fill(0.0);
        }
        block.parameter_events.clear();
        block.parameter_events.extend_from_slice(parameter_events);
        block.note_events.clear();
        block.note_events.extend_from_slice(note_events);
        match self.requests.push(block) {
            Ok(()) => {
                self.counters.submitted.fetch_add(1, Ordering::Relaxed);
            }
            Err(block) => {
                self.pool.push(block);
                self.counters.queue_drops.fetch_add(1, Ordering::Relaxed);
            }
        }
        self.sequence = self.sequence.wrapping_add(1);
        Ok(())
    }
}

// off the audio thread
impl Drop for Bridge {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop<P: BlockProcessor>(
    mut processor: P,
    mut requests: Consumer<Block>,
    mut responses: Producer<Block>,
    running: &AtomicBool,
    counters: &Counters,
) {
    let mut idle = 0_u32;
    while running.load(Ordering::Acquire) || !requests.is_empty() {
        let Some(mut block) = requests.pop() else {
            idle = idle.wrapping_add(1);
            if idle < 256 {
                spin_loop();
            } else {
                thread::yield_now();
            }
            continue;
        };
        idle = 0;
        let input = block
            .has_input
            .then_some((&block.input_left[..], &block.input_right[..]));
        let succeeded = processor.process_block(
            input,
            &mut block.output_left,
            &mut block.output_right,
            &block.parameter_events,
            &block.note_events,
        );
        if !succeeded {
            block.output_left.fill(0.0);
            block.output_right.fill(0.0);
            counters.worker_failures.fetch_add(1, Ordering::Relaxed);
            running.store(false, Ordering::Release);
        }
        counters.completed.fetch_add(1, Ordering::Relaxed);
        loop {
            match responses.push(block) {
                Ok(()) => break,
                Err(returned) if running.load(Ordering::Acquire) => {
                    block = returned;
                    thread::yield_now();
                }
                Err(_) => return,
            }
        }
        if !succeeded {
            return;
        }
    }
}

// back on the audio thread

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    struct Gain(f32);

    impl BlockProcessor for Gain {
        fn process_block(
            &mut self,
            input: Option<(&[f32], &[f32])>,
            output_left: &mut [f32],
            output_right: &mut [f32],
            _: &[ParameterEvent],
            _: &[NoteEvent],
        ) -> bool {
            let Some((left, right)) = input else {
                output_left.fill(0.0);
                output_right.fill(0.0);
                return true;
            };
            for frame in 0..left.len() {
                output_left[frame] = left[frame] * self.0;
                output_right[frame] = right[frame] * self.0;
            }
            true
        }
    }

    fn wait_for(bridge: &Bridge, completed: u64) {
        let deadline = Instant::now() + Duration::from_secs(1);
        while bridge.completed_blocks() < completed && Instant::now() < deadline {
            thread::yield_now();
        }
        assert!(bridge.completed_blocks() >= completed);
    }

    #[test]
    fn processed_audio_arrives_one_block_later() {
        let mut bridge = Bridge::new(Gain(2.0), 4, 2, 7).unwrap();
        assert_eq!(bridge.latency_frames(), 11);
        let left = [1.0, 2.0, 3.0, 4.0];
        let right = [-1.0, -2.0, -3.0, -4.0];
        let mut out_left = [9.0; 4];
        let mut out_right = [9.0; 4];
        bridge
            .process_stereo(
                Some((&left, &right)),
                &mut out_left,
                &mut out_right,
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(out_left, [0.0; 4]);
        wait_for(&bridge, 1);
        bridge
            .process_stereo(
                Some((&left, &right)),
                &mut out_left,
                &mut out_right,
                &[],
                &[],
            )
            .unwrap();
        assert_eq!(out_left, [2.0, 4.0, 6.0, 8.0]);
        assert_eq!(out_right, [-2.0, -4.0, -6.0, -8.0]);
        assert_eq!(bridge.submitted_blocks(), 2);
        assert_eq!(bridge.underruns(), 0);
    }

    #[test]
    fn a_late_block_uses_the_delayed_dry_signal() {
        struct Held {
            release: Arc<AtomicBool>,
        }
        impl BlockProcessor for Held {
            fn process_block(
                &mut self,
                input: Option<(&[f32], &[f32])>,
                output_left: &mut [f32],
                output_right: &mut [f32],
                _: &[ParameterEvent],
                _: &[NoteEvent],
            ) -> bool {
                while !self.release.load(Ordering::Acquire) {
                    spin_loop();
                }
                let (left, right) = input.unwrap();
                output_left.copy_from_slice(left);
                output_right.copy_from_slice(right);
                true
            }
        }
        let release = Arc::new(AtomicBool::new(false));
        let mut bridge = Bridge::new(
            Held {
                release: Arc::clone(&release),
            },
            2,
            2,
            0,
        )
        .unwrap();
        let first = [0.25, 0.5];
        let second = [0.75, 1.0];
        let mut left = [0.0; 2];
        let mut right = [0.0; 2];
        bridge
            .process_stereo(Some((&first, &first)), &mut left, &mut right, &[], &[])
            .unwrap();
        bridge
            .process_stereo(Some((&second, &second)), &mut left, &mut right, &[], &[])
            .unwrap();
        assert_eq!(left, first);
        assert_eq!(bridge.underruns(), 1);
        release.store(true, Ordering::Release);
    }

    #[test]
    fn invalid_configuration_and_blocks_are_rejected() {
        assert!(matches!(
            Bridge::new(Gain(1.0), 0, 2, 0),
            Err(Error::InvalidConfiguration)
        ));
        let mut bridge = Bridge::new(Gain(1.0), 2, 2, 0).unwrap();
        let mut left = [0.0; 1];
        let mut right = [0.0; 1];
        assert_eq!(
            bridge.process_stereo(None, &mut left, &mut right, &[], &[]),
            Err(Error::InvalidBlock)
        );
        let mut left = [0.0; 2];
        let mut right = [0.0; 2];
        let invalid = [ParameterEvent {
            sample_offset: 2,
            identifier: 7,
            value: 0.5,
        }];
        assert_eq!(
            bridge.process_stereo(None, &mut left, &mut right, &invalid, &[]),
            Err(Error::InvalidBlock)
        );
        assert_eq!(bridge.submitted_blocks(), 0);
    }

    #[test]
    fn a_processor_failure_stops_new_submissions() {
        struct Fails;
        impl BlockProcessor for Fails {
            fn process_block(
                &mut self,
                _: Option<(&[f32], &[f32])>,
                _: &mut [f32],
                _: &mut [f32],
                _: &[ParameterEvent],
                _: &[NoteEvent],
            ) -> bool {
                false
            }
        }
        let mut bridge = Bridge::new(Fails, 2, 2, 0).unwrap();
        let mut left = [0.0; 2];
        let mut right = [0.0; 2];
        bridge
            .process_stereo(None, &mut left, &mut right, &[], &[])
            .unwrap();
        wait_for(&bridge, 1);
        assert!(!bridge.is_running());
        assert_eq!(bridge.worker_failures(), 1);
        bridge
            .process_stereo(None, &mut left, &mut right, &[], &[])
            .unwrap();
        assert_eq!(bridge.submitted_blocks(), 1);
        assert_eq!(bridge.queue_drops(), 1);
    }
}
