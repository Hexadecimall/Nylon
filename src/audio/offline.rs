//! Rendering without a device.
//!
//! The offline backend runs a [`Renderer`](super::Renderer) as fast as the
//! machine allows rather than against a device clock. That is what a
//! bounce needs, and it is what the tests use: the same renderer that
//! feeds the speakers produces the file, so an export matches what was
//! heard.
//!
//! A stream here holds its renderer and produces audio only when asked,
//! through [`OfflineStream::render_into`] or
//! [`OfflineStream::render_frames`]. Nothing runs on another thread, so a
//! test can step a render one block at a time and inspect the result.

use super::{
    AudioError, Backend, BlockTiming, Capturer, DeviceId, DeviceInfo, Direction, InputBackend,
    Name, Rates, Renderer, SUPPORTED_RATES, Stream, StreamConfig,
};

/// Identifier of the single output device the offline backend offers.
pub const DEVICE: DeviceId = DeviceId(0);
/// Identifier of the single input device it offers.
pub const INPUT_DEVICE: DeviceId = DeviceId(1);

/// A backend with one device that never blocks.
#[derive(Clone, Copy, Debug, Default)]
pub struct OfflineBackend;

impl OfflineBackend {
    /// A backend offering one stereo output device.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Description of the output device this backend offers.
    #[must_use]
    pub fn device_info() -> DeviceInfo {
        DeviceInfo {
            id: DEVICE,
            name: Name::truncated("Offline"),
            direction: Direction::Output,
            channels: 2,
            rates: Rates::from_slice(&SUPPORTED_RATES),
            is_default: true,
        }
    }

    /// Description of the input device this backend offers.
    #[must_use]
    pub fn input_device_info() -> DeviceInfo {
        DeviceInfo {
            id: INPUT_DEVICE,
            name: Name::truncated("Offline input"),
            direction: Direction::Input,
            channels: 2,
            rates: Rates::from_slice(&SUPPORTED_RATES),
            is_default: true,
        }
    }
}

impl Backend for OfflineBackend {
    type Stream = OfflineStream;

    fn name(&self) -> &'static str {
        "Offline"
    }

    fn devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        if out.is_empty() {
            return Ok(0);
        }
        out[0] = Self::device_info();
        Ok(1)
    }

    fn default_output(&self) -> Result<DeviceId, AudioError> {
        Ok(DEVICE)
    }

    fn open_output<R: Renderer + 'static>(
        &self,
        config: StreamConfig,
        renderer: R,
    ) -> Result<Self::Stream, AudioError> {
        if config.device != DEVICE {
            return Err(AudioError::DeviceMissing);
        }
        config.validate()?;
        let mut renderer = renderer;
        renderer.prepare(config);
        // Boxing happens here, on the control thread, before any audio is
        // produced; the render path itself never allocates.
        Ok(OfflineStream {
            config,
            renderer: Some(Box::new(renderer)),
            running: false,
            frames: 0,
        })
    }
}

impl InputBackend for OfflineBackend {
    type Capture = OfflineCapture;

    fn input_devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError> {
        if out.is_empty() {
            return Ok(0);
        }
        out[0] = Self::input_device_info();
        Ok(1)
    }

    fn default_input(&self) -> Result<DeviceId, AudioError> {
        Ok(INPUT_DEVICE)
    }

    fn open_input<C: Capturer + 'static>(
        &self,
        config: StreamConfig,
        capturer: C,
    ) -> Result<Self::Capture, AudioError> {
        if config.device != INPUT_DEVICE {
            return Err(AudioError::DeviceMissing);
        }
        config.validate()?;
        let mut capturer = capturer;
        capturer.prepare(config);
        // Boxing happens on the control thread, before any audio moves.
        Ok(OfflineCapture {
            config,
            capturer: Some(Box::new(capturer)),
            running: false,
            frames: 0,
        })
    }
}

/// A stream that captures whatever it is handed, rather than reading a
/// device. It is what a test uses to drive a recording path, and what an
/// import feeds when material arrives from a file rather than a jack.
pub struct OfflineCapture {
    config: StreamConfig,
    // Held in an option so the capturer can be taken back once the stream
    // is finished with, which is how a test reads what it recorded.
    capturer: Option<Box<dyn Capturer>>,
    running: bool,
    frames: u64,
}

impl OfflineCapture {
    /// Hands `input` to the capturer as though the device had delivered
    /// it. Returns the frames taken.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when the stream is not running
    /// and [`AudioError::Unsupported`] when the buffer is longer than the
    /// configured block.
    pub fn capture_from(&mut self, input: &[[f32; 2]]) -> Result<usize, AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        if input.len() > self.config.block_frames {
            return Err(AudioError::Unsupported("block size"));
        }
        let Some(capturer) = self.capturer.as_mut() else {
            return Err(AudioError::WrongState);
        };
        let timing = BlockTiming {
            frame: self.frames,
            dropouts: 0,
        };
        capturer.capture(input, timing);
        self.frames += input.len() as u64;
        Ok(input.len())
    }

    /// Hands over `input` one block at a time.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when the stream is not running.
    pub fn capture_all(&mut self, input: &[[f32; 2]]) -> Result<usize, AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        let block = self.config.block_frames;
        let mut taken = 0;
        while taken < input.len() {
            let end = (taken + block).min(input.len());
            self.capture_from(&input[taken..end])?;
            taken = end;
        }
        Ok(taken)
    }

    /// Returns the capturer, ending the stream.
    #[must_use]
    pub fn into_capturer(mut self) -> Option<Box<dyn Capturer>> {
        self.running = false;
        self.capturer.take()
    }
}

impl Stream for OfflineCapture {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.running {
            return Err(AudioError::WrongState);
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn frames_rendered(&self) -> u64 {
        self.frames
    }

    fn dropouts(&self) -> u64 {
        // Nothing here runs against a clock, so nothing arrives late.
        0
    }
}

/// A stream that renders on demand.
pub struct OfflineStream {
    config: StreamConfig,
    // Held in an option so the renderer can be taken back when the stream
    // is finished with, which a bounce needs in order to read its results.
    renderer: Option<Box<dyn Renderer>>,
    running: bool,
    frames: u64,
}

impl OfflineStream {
    /// Renders into `output`, which must be a whole number of frames no
    /// longer than the configured block. Returns the frames written.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when the stream is not running
    /// and [`AudioError::Unsupported`] when the buffer is longer than the
    /// configured block.
    pub fn render_into(&mut self, output: &mut [[f32; 2]]) -> Result<usize, AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        if output.len() > self.config.block_frames {
            return Err(AudioError::Unsupported("block size"));
        }
        let Some(renderer) = self.renderer.as_mut() else {
            return Err(AudioError::WrongState);
        };
        let timing = BlockTiming {
            frame: self.frames,
            dropouts: 0,
        };
        renderer.render(output, timing);
        self.frames += output.len() as u64;
        Ok(output.len())
    }

    /// Renders `frames` frames into `output`, one block at a time.
    ///
    /// `output` must hold at least `frames` frames. Blocks are the
    /// configured size except the last, which carries the remainder.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when the stream is not running
    /// and [`AudioError::Unsupported`] when `output` is too short.
    pub fn render_frames(
        &mut self,
        frames: usize,
        output: &mut [[f32; 2]],
    ) -> Result<usize, AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        if output.len() < frames {
            return Err(AudioError::Unsupported("output length"));
        }
        let block = self.config.block_frames;
        let mut written = 0;
        while written < frames {
            let end = (written + block).min(frames);
            self.render_into(&mut output[written..end])?;
            written = end;
        }
        Ok(written)
    }

    /// Returns the renderer, ending the stream. Stopping first is not
    /// required; a running stream is stopped as it is taken apart.
    #[must_use]
    pub fn into_renderer(mut self) -> Option<Box<dyn Renderer>> {
        self.running = false;
        self.renderer.take()
    }
}

impl Stream for OfflineStream {
    fn config(&self) -> StreamConfig {
        self.config
    }

    fn start(&mut self) -> Result<(), AudioError> {
        if self.running {
            return Err(AudioError::WrongState);
        }
        self.running = true;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if !self.running {
            return Err(AudioError::WrongState);
        }
        self.running = false;
        Ok(())
    }

    fn is_running(&self) -> bool {
        self.running
    }

    fn frames_rendered(&self) -> u64 {
        self.frames
    }

    fn dropouts(&self) -> u64 {
        // Nothing here runs against a clock, so nothing is ever late.
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::MIN_BLOCK;
    /// A capturer that keeps what it was handed, so a test can look at it.
    struct Recording {
        frames: Vec<[f32; 2]>,
        blocks: usize,
        prepared: Option<StreamConfig>,
        first_frame: Option<u64>,
    }

    impl Recording {
        fn new() -> Self {
            Self {
                frames: Vec::new(),
                blocks: 0,
                prepared: None,
                first_frame: None,
            }
        }
    }

    impl Capturer for Recording {
        fn capture(&mut self, input: &[[f32; 2]], timing: BlockTiming) {
            if self.first_frame.is_none() {
                self.first_frame = Some(timing.frame);
            }
            self.frames.extend_from_slice(input);
            self.blocks += 1;
        }

        fn prepare(&mut self, config: StreamConfig) {
            self.prepared = Some(config);
        }
    }

    /// The smallest block a stream may be opened with, which keeps the
    /// tests below honest about splitting.
    fn input_config(block: usize) -> StreamConfig {
        StreamConfig {
            device: INPUT_DEVICE,
            sample_rate: 48_000,
            block_frames: block,
            channels: 2,
        }
    }

    #[test]
    fn an_input_device_is_offered_beside_the_output() {
        let backend = OfflineBackend::new();
        let mut devices = [OfflineBackend::device_info(); 2];
        assert_eq!(backend.input_devices(&mut devices).unwrap(), 1);
        assert_eq!(devices[0].id, INPUT_DEVICE);
        assert_eq!(devices[0].direction, Direction::Input);
        assert_ne!(devices[0].id, DEVICE, "the two devices are not the same");
        assert_eq!(backend.default_input().unwrap(), INPUT_DEVICE);
        // A caller with no room is told nothing was written.
        assert_eq!(backend.input_devices(&mut []).unwrap(), 0);
    }

    #[test]
    fn a_capture_stream_hands_over_what_it_is_given() {
        let backend = OfflineBackend::new();
        let mut stream = backend
            .open_input(input_config(MIN_BLOCK), Recording::new())
            .expect("the input would not open");
        assert_eq!(stream.config().block_frames, MIN_BLOCK);
        assert!(!stream.is_running());
        assert_eq!(stream.frames_rendered(), 0);
        assert_eq!(stream.dropouts(), 0);
        assert!(!stream.is_lost());

        // Nothing is taken until the stream runs.
        assert_eq!(
            stream.capture_from(&[[0.0, 0.0]]),
            Err(AudioError::WrongState)
        );
        assert!(stream.start().is_ok());
        assert_eq!(stream.start(), Err(AudioError::WrongState));

        let material: Vec<[f32; 2]> = (0..MIN_BLOCK * 2 + 3)
            .map(|index| [index as f32, -(index as f32)])
            .collect();
        let count = material.len();
        assert_eq!(stream.capture_all(&material).unwrap(), count);
        assert_eq!(stream.frames_rendered(), count as u64);
        // A block longer than the configured one is refused rather than
        // split behind the caller's back.
        assert_eq!(
            stream.capture_from(&material),
            Err(AudioError::Unsupported("block size"))
        );
        assert!(stream.stop().is_ok());
        assert_eq!(stream.stop(), Err(AudioError::WrongState));

        // The capturer comes back so its owner can read what it took; what
        // it took is checked in the test below.
        assert!(stream.into_capturer().is_some());
    }

    #[test]
    fn capture_keeps_every_frame_in_order_and_counts_the_blocks() {
        let backend = OfflineBackend::new();
        // Two whole blocks and a short one, so the split is visible.
        let material: Vec<[f32; 2]> = (0..MIN_BLOCK * 2 + 1)
            .map(|index| [index as f32, index as f32 * 0.5])
            .collect();

        // The capturer is kept here rather than inside the stream so the
        // test can read it afterwards.
        let shared = std::sync::Arc::new(std::sync::Mutex::new(Recording::new()));
        struct Forwarding(std::sync::Arc<std::sync::Mutex<Recording>>);
        impl Capturer for Forwarding {
            fn capture(&mut self, input: &[[f32; 2]], timing: BlockTiming) {
                self.0.lock().unwrap().capture(input, timing);
            }
            fn prepare(&mut self, config: StreamConfig) {
                self.0.lock().unwrap().prepare(config);
            }
        }

        let mut stream = backend
            .open_input(
                input_config(MIN_BLOCK),
                Forwarding(std::sync::Arc::clone(&shared)),
            )
            .expect("the input would not open");
        assert!(stream.start().is_ok());
        assert_eq!(stream.capture_all(&material).unwrap(), material.len());

        let recording = shared.lock().unwrap();
        assert_eq!(recording.frames, material, "the material arrived changed");
        assert_eq!(recording.blocks, 3, "the material was not split by block");
        assert_eq!(recording.first_frame, Some(0));
        assert_eq!(
            recording.prepared.map(|config| config.block_frames),
            Some(MIN_BLOCK),
            "the capturer was not told what it was opened with"
        );
    }

    #[test]
    fn an_input_stream_refuses_the_wrong_device_and_a_bad_rate() {
        let backend = OfflineBackend::new();
        let wrong = StreamConfig {
            device: DEVICE,
            ..input_config(MIN_BLOCK)
        };
        assert!(matches!(
            backend.open_input(wrong, Recording::new()),
            Err(AudioError::DeviceMissing)
        ));
        let unusable = StreamConfig {
            sample_rate: 0,
            ..input_config(MIN_BLOCK)
        };
        assert!(backend.open_input(unusable, Recording::new()).is_err());
    }

    /// A renderer that writes a rising ramp so the tests can tell which
    /// frame each sample came from, and records what it was prepared with.
    struct Counting {
        prepared: Option<StreamConfig>,
        calls: usize,
        last_timing: BlockTiming,
    }

    impl Counting {
        fn new() -> Self {
            Self {
                prepared: None,
                calls: 0,
                last_timing: BlockTiming::default(),
            }
        }
    }

    impl Renderer for Counting {
        fn render(&mut self, output: &mut [[f32; 2]], timing: BlockTiming) {
            self.calls += 1;
            self.last_timing = timing;
            for (index, frame) in output.iter_mut().enumerate() {
                let position = timing.frame + index as u64;
                *frame = [position as f32, -(position as f32)];
            }
        }

        fn prepare(&mut self, config: StreamConfig) {
            self.prepared = Some(config);
        }
    }

    fn config() -> StreamConfig {
        StreamConfig {
            device: DEVICE,
            sample_rate: 48_000,
            block_frames: 64,
            channels: 2,
        }
    }

    #[test]
    fn the_backend_offers_one_default_output() {
        let backend = OfflineBackend::new();
        assert_eq!(backend.name(), "Offline");
        assert_eq!(backend.default_output(), Ok(DEVICE));

        let mut devices = [OfflineBackend::device_info(); 4];
        assert_eq!(backend.devices(&mut devices), Ok(1));
        assert_eq!(devices[0].id, DEVICE);
        assert_eq!(devices[0].direction, Direction::Output);
        assert_eq!(devices[0].channels, 2);
        assert!(devices[0].is_default);
        assert!(devices[0].rates.contains(48_000));
        assert_eq!(devices[0].name.as_str(), "Offline");

        // A caller with no room is told nothing was written.
        assert_eq!(backend.devices(&mut []), Ok(0));
    }

    #[test]
    fn opening_prepares_the_renderer_with_the_granted_configuration() {
        let backend = OfflineBackend::new();
        let stream = backend.open_output(config(), Counting::new()).unwrap();
        assert_eq!(stream.config(), config());
        assert!(!stream.is_running());
        // An offline stream has no device to lose, and nothing between
        // the renderer and a listener to delay it.
        assert!(!stream.is_lost());
        assert_eq!(stream.latency_frames(), 0);
        assert_eq!(stream.frames_rendered(), 0);
        assert_eq!(stream.dropouts(), 0);
    }

    #[test]
    fn an_unknown_device_or_bad_configuration_is_refused() {
        let backend = OfflineBackend::new();
        let mut wrong_device = config();
        wrong_device.device = DeviceId(99);
        assert!(matches!(
            backend.open_output(wrong_device, Counting::new()),
            Err(AudioError::DeviceMissing)
        ));

        let mut wrong_rate = config();
        wrong_rate.sample_rate = 1_234;
        assert!(matches!(
            backend.open_output(wrong_rate, Counting::new()),
            Err(AudioError::Unsupported("sample rate"))
        ));
    }

    #[test]
    fn a_stopped_stream_renders_nothing() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        let mut output = [[9.0_f32, 9.0]; 64];
        assert_eq!(stream.render_into(&mut output), Err(AudioError::WrongState));
        // The buffer is untouched.
        assert_eq!(output[0], [9.0, 9.0]);
        assert_eq!(stream.frames_rendered(), 0);
    }

    #[test]
    fn starting_and_stopping_twice_is_refused() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        assert_eq!(stream.start(), Ok(()));
        assert!(stream.is_running());
        assert_eq!(stream.start(), Err(AudioError::WrongState));
        assert_eq!(stream.stop(), Ok(()));
        assert!(!stream.is_running());
        assert_eq!(stream.stop(), Err(AudioError::WrongState));
    }

    #[test]
    fn the_frame_counter_carries_across_blocks() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut first = [[0.0_f32; 2]; 64];
        let mut second = [[0.0_f32; 2]; 64];
        assert_eq!(stream.render_into(&mut first), Ok(64));
        assert_eq!(stream.render_into(&mut second), Ok(64));
        assert_eq!(stream.frames_rendered(), 128);
        assert_eq!(first[0][0], 0.0);
        assert_eq!(first[63][0], 63.0);
        assert_eq!(second[0][0], 64.0);
        assert_eq!(second[63][0], 127.0);
    }

    #[test]
    fn a_block_longer_than_configured_is_refused() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut too_long = [[0.0_f32; 2]; 65];
        assert_eq!(
            stream.render_into(&mut too_long),
            Err(AudioError::Unsupported("block size"))
        );
        assert_eq!(stream.frames_rendered(), 0);
    }

    #[test]
    fn a_shorter_block_is_accepted() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut partial = [[0.0_f32; 2]; 10];
        assert_eq!(stream.render_into(&mut partial), Ok(10));
        assert_eq!(stream.frames_rendered(), 10);
        assert_eq!(partial[9][0], 9.0);
    }

    #[test]
    fn a_run_of_frames_is_split_into_blocks_with_a_remainder() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        // 150 frames is two whole blocks of 64 and a remainder of 22.
        let mut output = vec![[0.0_f32; 2]; 150];
        assert_eq!(stream.render_frames(150, &mut output), Ok(150));
        assert_eq!(stream.frames_rendered(), 150);
        for (index, frame) in output.iter().enumerate() {
            assert_eq!(frame[0], index as f32, "{index}");
            assert_eq!(frame[1], -(index as f32), "{index}");
        }
        let renderer = stream.into_renderer().unwrap();
        drop(renderer);
    }

    #[test]
    fn rendering_a_run_into_a_short_buffer_is_refused() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut output = [[0.0_f32; 2]; 10];
        assert_eq!(
            stream.render_frames(64, &mut output),
            Err(AudioError::Unsupported("output length"))
        );
        assert_eq!(stream.frames_rendered(), 0);
    }

    #[test]
    fn rendering_no_frames_does_nothing() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut output: [[f32; 2]; 0] = [];
        assert_eq!(stream.render_frames(0, &mut output), Ok(0));
        assert_eq!(stream.frames_rendered(), 0);
    }

    #[test]
    fn the_renderer_can_be_taken_back_after_a_bounce() {
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let mut output = vec![[0.0_f32; 2]; 128];
        stream.render_frames(128, &mut output).unwrap();
        assert!(stream.into_renderer().is_some());
    }

    #[test]
    fn rendering_is_faster_than_the_audio_it_produces() {
        // The point of the offline backend: a bounce is not paced by a
        // clock. Ten seconds of audio must take far less than ten seconds.
        let backend = OfflineBackend::new();
        let mut stream = backend.open_output(config(), Counting::new()).unwrap();
        stream.start().unwrap();
        let frames = 48_000 * 10;
        let mut output = vec![[0.0_f32; 2]; frames];
        let begin = std::time::Instant::now();
        assert_eq!(stream.render_frames(frames, &mut output), Ok(frames));
        assert!(
            begin.elapsed() < std::time::Duration::from_secs(2),
            "ten seconds of audio took {:?}",
            begin.elapsed()
        );
    }
}
