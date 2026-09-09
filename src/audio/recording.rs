//! Real-time-safe capture into a WAVE file.
//!
//! The device callback copies bounded blocks into an SPSC queue. A file
//! thread drains that queue and performs every allocation and system call.

use std::fs::File;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread::JoinHandle;

use super::{AudioError, BlockTiming, Capturer, InputBackend, Stream, StreamConfig};
use crate::mixer::MAX_FRAMES;
use crate::spsc::{Consumer, Producer, SpscQueue};
use crate::wave::{Format, SampleFormat, WaveError, WaveWriter};

/// Default number of maximum-sized input blocks held between the audio and
/// file threads.
pub const DEFAULT_QUEUE_BLOCKS: usize = 256;
/// Largest queue accepted, limiting preallocated capture storage to 64 MiB.
pub const MAX_QUEUE_BLOCKS: usize = 4096;

#[derive(Clone, Copy)]
struct CaptureBlock {
    frames: [[f32; 2]; MAX_FRAMES],
    count: usize,
}

impl CaptureBlock {
    fn from_slice(input: &[[f32; 2]]) -> Self {
        let mut block = Self {
            frames: [[0.0; 2]; MAX_FRAMES],
            count: input.len(),
        };
        block.frames[..input.len()].copy_from_slice(input);
        block
    }
}

/// Counts material that could not enter a full recording queue.
#[derive(Default)]
struct Loss {
    blocks: AtomicU64,
    frames: AtomicU64,
}

/// Capture endpoint passed to a platform input stream.
struct RecordingCapturer {
    producer: Producer<CaptureBlock>,
    loss: Arc<Loss>,
}

impl Capturer for RecordingCapturer {
    fn capture(&mut self, input: &[[f32; 2]], _timing: BlockTiming) {
        for chunk in input.chunks(MAX_FRAMES) {
            let count = chunk.len() as u64;
            if self.producer.push(CaptureBlock::from_slice(chunk)).is_err() {
                self.loss.blocks.fetch_add(1, Ordering::Relaxed);
                self.loss.frames.fetch_add(count, Ordering::Relaxed);
            }
        }
    }
}

/// Why a recording could not be opened or completed.
#[derive(Debug)]
pub enum RecordingError {
    /// The audio host refused the input stream.
    Audio(AudioError),
    /// The destination could not be written.
    Wave(WaveError),
    /// The file thread could not be created or did not finish normally.
    Thread,
}

impl core::fmt::Display for RecordingError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Audio(error) => write!(formatter, "audio input: {error}"),
            Self::Wave(error) => write!(formatter, "recording file: {error}"),
            Self::Thread => formatter.write_str("the recording file thread failed"),
        }
    }
}

impl std::error::Error for RecordingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Audio(error) => Some(error),
            Self::Wave(error) => Some(error),
            Self::Thread => None,
        }
    }
}

impl From<AudioError> for RecordingError {
    fn from(error: AudioError) -> Self {
        Self::Audio(error)
    }
}

impl From<WaveError> for RecordingError {
    fn from(error: WaveError) -> Self {
        Self::Wave(error)
    }
}

/// Result of a finished recording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingReport {
    /// File containing the captured material.
    pub path: PathBuf,
    /// Frames written to the file.
    pub frames: u64,
    /// Frames per second stored in the file.
    pub sample_rate: u32,
    /// Blocks rejected because the queue was full.
    pub lost_blocks: u64,
    /// Frames contained by rejected blocks.
    pub lost_frames: u64,
}

/// An input stream connected to a background WAVE writer.
pub struct RecordingSession<S: Stream> {
    stream: Option<S>,
    writer: Option<JoinHandle<Result<u64, WaveError>>>,
    loss: Arc<Loss>,
    path: PathBuf,
    sample_rate: u32,
}

impl<S: Stream> RecordingSession<S> {
    /// Opens a stopped input stream and its destination.
    ///
    /// # Errors
    ///
    /// Returns an audio, file, or thread error when setup fails.
    pub fn open<B>(
        backend: &B,
        config: StreamConfig,
        destination: &Path,
        queue_blocks: usize,
    ) -> Result<Self, RecordingError>
    where
        B: InputBackend<Capture = S>,
    {
        validate_queue_size(queue_blocks)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)
            .map_err(WaveError::Io)?;
        match Self::open_file(backend, config, file, destination, queue_blocks) {
            Ok(session) => Ok(session),
            Err(error) => {
                let _ = std::fs::remove_file(destination);
                Err(error)
            }
        }
    }

    /// Opens a stopped input stream using an already reserved file.
    ///
    /// # Errors
    ///
    /// Returns an audio, file, or thread error when setup fails.
    pub fn open_file<B>(
        backend: &B,
        config: StreamConfig,
        file: File,
        destination: &Path,
        queue_blocks: usize,
    ) -> Result<Self, RecordingError>
    where
        B: InputBackend<Capture = S>,
    {
        validate_queue_size(queue_blocks)?;
        let (producer, consumer) = SpscQueue::with_capacity(queue_blocks);
        let loss = Arc::new(Loss::default());
        let capturer = RecordingCapturer {
            producer,
            loss: Arc::clone(&loss),
        };
        let stream = backend.open_input(config, capturer)?;
        let granted = stream.config();
        if granted.channels != 2 {
            return Err(RecordingError::Audio(AudioError::Unsupported(
                "channel count",
            )));
        }
        let format = Format {
            sample_rate: granted.sample_rate,
            channels: granted.channels,
            bits: 32,
            sample_format: SampleFormat::Float,
        };
        let writer = WaveWriter::new(file, format)?;
        let thread = std::thread::Builder::new()
            .name("nylon-recording".to_owned())
            .spawn(move || write_blocks(consumer, writer));
        let thread = match thread {
            Ok(thread) => thread,
            Err(_) => {
                let _ = std::fs::remove_file(destination);
                return Err(RecordingError::Thread);
            }
        };
        Ok(Self {
            stream: Some(stream),
            writer: Some(thread),
            loss,
            path: destination.to_path_buf(),
            sample_rate: granted.sample_rate,
        })
    }

    /// Opens a recording with the standard queue capacity.
    pub fn open_default<B>(
        backend: &B,
        config: StreamConfig,
        destination: &Path,
    ) -> Result<Self, RecordingError>
    where
        B: InputBackend<Capture = S>,
    {
        Self::open(backend, config, destination, DEFAULT_QUEUE_BLOCKS)
    }

    /// Starts the input stream.
    pub fn start(&mut self) -> Result<(), RecordingError> {
        self.stream
            .as_mut()
            .ok_or(RecordingError::Thread)?
            .start()
            .map_err(Into::into)
    }

    /// Stops the input stream while keeping the destination open.
    pub fn stop(&mut self) -> Result<(), RecordingError> {
        self.stream
            .as_mut()
            .ok_or(RecordingError::Thread)?
            .stop()
            .map_err(Into::into)
    }

    /// Accesses the input stream for device status and offline driving.
    pub fn stream_mut(&mut self) -> Option<&mut S> {
        self.stream.as_mut()
    }

    /// Accesses the input stream for device status.
    #[must_use]
    pub fn stream(&self) -> Option<&S> {
        self.stream.as_ref()
    }

    /// Stops capture, drains queued blocks, and finalizes the file.
    pub fn finish(mut self) -> Result<RecordingReport, RecordingError> {
        if self.stream.as_ref().is_some_and(Stream::is_running) {
            self.stop()?;
        }
        self.stream.take();
        let frames = self
            .writer
            .take()
            .ok_or(RecordingError::Thread)?
            .join()
            .map_err(|_| RecordingError::Thread)??;
        Ok(RecordingReport {
            path: self.path.clone(),
            frames,
            sample_rate: self.sample_rate,
            lost_blocks: self.loss.blocks.load(Ordering::Relaxed),
            lost_frames: self.loss.frames.load(Ordering::Relaxed),
        })
    }
}

fn validate_queue_size(queue_blocks: usize) -> Result<(), RecordingError> {
    if queue_blocks == 0 || queue_blocks > MAX_QUEUE_BLOCKS {
        Err(RecordingError::Audio(AudioError::Unsupported(
            "recording queue size",
        )))
    } else {
        Ok(())
    }
}

impl<S: Stream> Drop for RecordingSession<S> {
    fn drop(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            if stream.is_running() {
                let _ = stream.stop();
            }
            drop(stream);
        }
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
    }
}

fn write_blocks(
    mut consumer: Consumer<CaptureBlock>,
    mut writer: WaveWriter<File>,
) -> Result<u64, WaveError> {
    loop {
        if let Some(block) = consumer.pop() {
            writer.write_stereo(&block.frames[..block.count])?;
        } else if consumer.is_abandoned() {
            let frames = writer.frames_written();
            writer.finish()?;
            return Ok(frames);
        } else {
            std::thread::sleep(core::time::Duration::from_millis(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::offline::{INPUT_DEVICE, OfflineBackend, OfflineCapture};

    fn destination(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("nylon-recording-{}-{name}.wav", std::process::id()))
    }

    fn config() -> StreamConfig {
        StreamConfig {
            device: INPUT_DEVICE,
            sample_rate: 48_000,
            block_frames: 16,
            channels: 2,
        }
    }

    #[test]
    fn captured_blocks_reach_a_finished_wave_file() {
        let path = destination("finished");
        let _ = std::fs::remove_file(&path);
        let mut session =
            RecordingSession::<OfflineCapture>::open(&OfflineBackend::new(), config(), &path, 4)
                .unwrap();
        session.start().unwrap();
        let input = [[0.25, -0.5]; 16];
        session.stream_mut().unwrap().capture_from(&input).unwrap();
        let report = session.finish().unwrap();
        let decoded = crate::wave::read(File::open(&path).unwrap()).unwrap();
        assert_eq!(report.frames, 16);
        assert_eq!(report.sample_rate, 48_000);
        assert_eq!(report.lost_frames, 0);
        assert_eq!(decoded.frames(), 16);
        assert_eq!(&decoded.samples[..4], &[0.25, -0.5, 0.25, -0.5]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_full_queue_reports_every_rejected_frame() {
        let (producer, _consumer) = SpscQueue::with_capacity(1);
        let loss = Arc::new(Loss::default());
        let mut capturer = RecordingCapturer {
            producer,
            loss: Arc::clone(&loss),
        };
        capturer.capture(&[[0.0; 2]; 16], BlockTiming::default());
        capturer.capture(&[[0.0; 2]; 7], BlockTiming::default());
        assert_eq!(loss.blocks.load(Ordering::Relaxed), 1);
        assert_eq!(loss.frames.load(Ordering::Relaxed), 7);
    }

    #[test]
    fn unusable_queue_sizes_are_refused() {
        let path = destination("queue-size");
        let backend = OfflineBackend::new();
        assert!(matches!(
            RecordingSession::<OfflineCapture>::open(&backend, config(), &path, 0),
            Err(RecordingError::Audio(AudioError::Unsupported(_)))
        ));
        assert!(matches!(
            RecordingSession::<OfflineCapture>::open(
                &backend,
                config(),
                &path,
                MAX_QUEUE_BLOCKS + 1
            ),
            Err(RecordingError::Audio(AudioError::Unsupported(_)))
        ));
    }

    #[test]
    fn opening_never_replaces_an_existing_file() {
        let path = destination("existing");
        std::fs::write(&path, b"keep").unwrap();
        assert!(
            RecordingSession::<OfflineCapture>::open(&OfflineBackend::new(), config(), &path, 4)
                .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"keep");
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_refused_input_leaves_no_new_file() {
        let path = destination("refused");
        let _ = std::fs::remove_file(&path);
        let mut invalid = config();
        invalid.device = super::super::DeviceId(99);
        assert!(
            RecordingSession::<OfflineCapture>::open(&OfflineBackend::new(), invalid, &path, 4)
                .is_err()
        );
        assert!(!path.exists());
    }
}
