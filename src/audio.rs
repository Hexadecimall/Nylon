//! Audio device interface and the offline renderer.
//!
//! A [`Backend`] enumerates devices and opens a stream. The stream calls a
//! [`Renderer`] from whatever thread the platform provides, once per
//! block, and that call is subject to every real-time rule: no allocation,
//! no locks, no syscalls.
//!
//! The offline backend in [`offline`] runs the same renderer as fast as
//! the machine allows, which is what a bounce needs and what the tests
//! use. A platform backend renders in real time against a device clock.

#[cfg(target_os = "linux")]
pub mod alsa;
#[cfg(target_os = "macos")]
pub mod coreaudio;
pub mod offline;

use core::fmt;

/// Sample rates a device may be asked for.
pub const SUPPORTED_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
/// Smallest block a stream may use.
pub const MIN_BLOCK: usize = 16;
/// Largest block a stream may use.
pub const MAX_BLOCK: usize = crate::mixer::MAX_FRAMES;
/// Longest device name kept, in bytes of UTF-8.
pub const MAX_NAME: usize = 128;
/// Most sample rates recorded per device.
pub const MAX_RATES: usize = 8;

/// A short, fixed-capacity name.
///
/// Device names come from the host and are only ever read, so they are
/// stored inline rather than allocated; a name longer than [`MAX_NAME`] is
/// truncated at a character boundary.
#[derive(Clone, Copy)]
pub struct Name {
    bytes: [u8; MAX_NAME],
    length: usize,
}

impl Name {
    /// An empty name.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; MAX_NAME],
            length: 0,
        }
    }

    /// A name holding as much of `text` as fits, cut at a character
    /// boundary so the result is always valid UTF-8.
    #[must_use]
    pub fn truncated(text: &str) -> Self {
        let mut length = text.len().min(MAX_NAME);
        while length > 0 && !text.is_char_boundary(length) {
            length -= 1;
        }
        let mut bytes = [0; MAX_NAME];
        bytes[..length].copy_from_slice(&text.as_bytes()[..length]);
        Self { bytes, length }
    }

    /// The name as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // SAFETY: `truncated` only ever stores a prefix that ends on a
        // character boundary, so the bytes remain valid UTF-8.
        core::str::from_utf8(&self.bytes[..self.length]).unwrap_or("")
    }

    /// Whether the name is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

impl Default for Name {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for Name {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Name {}

impl fmt::Debug for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), formatter)
    }
}

impl fmt::Display for Name {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Sample rates a device accepts, most preferred first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rates {
    values: [u32; MAX_RATES],
    length: usize,
}

impl Rates {
    /// An empty list.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            values: [0; MAX_RATES],
            length: 0,
        }
    }

    /// A list holding as many of `rates` as fit.
    #[must_use]
    pub fn from_slice(rates: &[u32]) -> Self {
        let mut list = Self::new();
        for rate in rates {
            if list.push(*rate).is_err() {
                break;
            }
        }
        list
    }

    /// Appends a rate. Returns it back when the list is full.
    ///
    /// # Errors
    ///
    /// Returns `Err(rate)` when [`MAX_RATES`] are already recorded.
    pub fn push(&mut self, rate: u32) -> Result<(), u32> {
        if self.length == MAX_RATES {
            return Err(rate);
        }
        self.values[self.length] = rate;
        self.length += 1;
        Ok(())
    }

    /// The rates recorded.
    #[must_use]
    pub fn as_slice(&self) -> &[u32] {
        &self.values[..self.length]
    }

    /// Whether `rate` is one of them.
    #[must_use]
    pub fn contains(&self, rate: u32) -> bool {
        self.as_slice().contains(&rate)
    }

    /// Number of rates recorded.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.length
    }

    /// Whether no rate is recorded.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.length == 0
    }
}

/// Which way audio flows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Into the application, from a microphone or input jack.
    Input,
    /// Out of the application, to speakers or an output jack.
    Output,
}

/// Identifier of a device, unique within one backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);

/// A device the host offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Identifier used to reopen the same device later.
    pub id: DeviceId,
    /// Name to show a person.
    pub name: Name,
    /// Direction the device carries.
    pub direction: Direction,
    /// Channels the device provides.
    pub channels: u16,
    /// Rates the device accepts, most preferred first.
    pub rates: Rates,
    /// Whether the host reports this as the default device.
    pub is_default: bool,
}

/// What a stream was asked to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamConfig {
    /// Device to open.
    pub device: DeviceId,
    /// Frames per second.
    pub sample_rate: u32,
    /// Frames the renderer is given at a time.
    pub block_frames: usize,
    /// Output channels to produce. Two is stereo.
    pub channels: u16,
}

impl StreamConfig {
    /// Rejects a configuration no stream could honor.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::Unsupported`] naming the field at fault.
    pub fn validate(&self) -> Result<(), AudioError> {
        if !SUPPORTED_RATES.contains(&self.sample_rate) {
            return Err(AudioError::Unsupported("sample rate"));
        }
        if self.block_frames < MIN_BLOCK || self.block_frames > MAX_BLOCK {
            return Err(AudioError::Unsupported("block size"));
        }
        if self.channels == 0 || self.channels > 2 {
            return Err(AudioError::Unsupported("channel count"));
        }
        Ok(())
    }

    /// Seconds of audio in one block.
    #[must_use]
    pub fn block_seconds(&self) -> f64 {
        f64::from(self.block_frames as u32) / f64::from(self.sample_rate)
    }
}

/// Why an audio operation failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// No device matched the identifier.
    DeviceMissing,
    /// The configuration names something the device cannot do. The field
    /// is the part at fault.
    Unsupported(&'static str),
    /// The stream is already running, or is not running when it must be.
    WrongState,
    /// The host refused the request.
    Host(&'static str),
}

impl fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeviceMissing => formatter.write_str("no such audio device"),
            Self::Unsupported(field) => write!(formatter, "unsupported {field}"),
            Self::WrongState => formatter.write_str("the stream is in the wrong state"),
            Self::Host(reason) => write!(formatter, "the audio host refused: {reason}"),
        }
    }
}

impl core::error::Error for AudioError {}

/// Timing a stream reports with each block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BlockTiming {
    /// Frames the stream produced before this block.
    pub frame: u64,
    /// Blocks the stream dropped because a callback ran long. Nonzero
    /// means the listener heard a gap.
    pub dropouts: u64,
}

/// Produces audio for a stream.
///
/// The implementation runs on the audio thread. It must not allocate,
/// lock, block, or make system calls, and it must fill every frame it is
/// given: a stream does not clear the buffer first.
pub trait Renderer: Send {
    /// Fills `output` with `output.len()` stereo frames.
    fn render(&mut self, output: &mut [[f32; 2]], timing: BlockTiming);

    /// Called before the first block with the configuration granted, which
    /// may differ from the one requested. Preparation that allocates must
    /// already have happened.
    fn prepare(&mut self, _config: StreamConfig) {}
}

/// A source of audio devices.
pub trait Backend {
    /// The stream type this backend opens.
    type Stream: Stream;

    /// Name of the host interface.
    fn name(&self) -> &'static str;

    /// Lists devices, writing at most `out.len()` entries and returning
    /// how many were written.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::Host`] when the host cannot be queried.
    fn devices(&self, out: &mut [DeviceInfo]) -> Result<usize, AudioError>;

    /// Identifier of the default output device.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::DeviceMissing`] when the host has none.
    fn default_output(&self) -> Result<DeviceId, AudioError>;

    /// Opens an output stream, created stopped.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError`] when the device or configuration is refused.
    fn open_output<R: Renderer + 'static>(
        &self,
        config: StreamConfig,
        renderer: R,
    ) -> Result<Self::Stream, AudioError>;
}

/// A stream that has been opened.
pub trait Stream {
    /// Configuration the stream is running with, which may differ from the
    /// one requested if the device rounded it.
    fn config(&self) -> StreamConfig;

    /// Starts producing audio.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when already running.
    fn start(&mut self) -> Result<(), AudioError>;

    /// Stops producing audio. The renderer is not called again until the
    /// stream is started.
    ///
    /// # Errors
    ///
    /// Returns [`AudioError::WrongState`] when not running.
    fn stop(&mut self) -> Result<(), AudioError>;

    /// Whether audio is being produced.
    fn is_running(&self) -> bool;

    /// Frames produced since the stream was opened.
    fn frames_rendered(&self) -> u64;

    /// Blocks dropped because a callback ran past its deadline.
    fn dropouts(&self) -> u64;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_holds_text_and_truncates_at_a_boundary() {
        let short = Name::truncated("Built-in Output");
        assert_eq!(short.as_str(), "Built-in Output");
        assert!(!short.is_empty());
        assert_eq!(Name::new().as_str(), "");
        assert!(Name::default().is_empty());

        let long = "x".repeat(MAX_NAME + 40);
        assert_eq!(Name::truncated(&long).as_str().len(), MAX_NAME);

        // A multi-byte character straddling the limit is dropped whole.
        let padded = format!("{}\u{00e9}\u{00e9}", "y".repeat(MAX_NAME - 1));
        let name = Name::truncated(&padded);
        assert_eq!(name.as_str().len(), MAX_NAME - 1);
        assert!(name.as_str().chars().all(|character| character == 'y'));
    }

    #[test]
    fn names_compare_and_print_by_their_text() {
        assert_eq!(Name::truncated("same"), Name::truncated("same"));
        assert_ne!(Name::truncated("one"), Name::truncated("other"));
        assert_eq!(Name::truncated("shown").to_string(), "shown");
        assert_eq!(format!("{:?}", Name::truncated("shown")), "\"shown\"");
    }

    #[test]
    fn rates_fill_and_stop_at_capacity() {
        let mut rates = Rates::new();
        assert!(rates.is_empty());
        for rate in &SUPPORTED_RATES {
            assert_eq!(rates.push(*rate), Ok(()));
        }
        assert_eq!(rates.len(), SUPPORTED_RATES.len());
        assert!(rates.contains(48_000));
        assert!(!rates.contains(22_050));
        assert_eq!(rates.as_slice(), &SUPPORTED_RATES);

        let overflowing: [u32; MAX_RATES + 3] = [48_000; MAX_RATES + 3];
        let full = Rates::from_slice(&overflowing);
        assert_eq!(full.len(), MAX_RATES);
        let mut one_more = full;
        assert_eq!(one_more.push(96_000), Err(96_000));
    }

    #[test]
    fn a_configuration_is_checked_against_what_a_stream_can_do() {
        let good = StreamConfig {
            device: DeviceId(1),
            sample_rate: 48_000,
            block_frames: 256,
            channels: 2,
        };
        assert_eq!(good.validate(), Ok(()));
        assert!((good.block_seconds() - 256.0 / 48_000.0).abs() < 1e-12);

        let mut bad = good;
        bad.sample_rate = 12_345;
        assert_eq!(bad.validate(), Err(AudioError::Unsupported("sample rate")));

        let mut bad = good;
        bad.block_frames = MIN_BLOCK - 1;
        assert_eq!(bad.validate(), Err(AudioError::Unsupported("block size")));
        bad.block_frames = MAX_BLOCK + 1;
        assert_eq!(bad.validate(), Err(AudioError::Unsupported("block size")));

        let mut bad = good;
        bad.channels = 0;
        assert_eq!(
            bad.validate(),
            Err(AudioError::Unsupported("channel count"))
        );
        bad.channels = 6;
        assert_eq!(
            bad.validate(),
            Err(AudioError::Unsupported("channel count"))
        );
    }

    #[test]
    fn every_supported_rate_is_accepted() {
        for rate in SUPPORTED_RATES {
            let config = StreamConfig {
                device: DeviceId(0),
                sample_rate: rate,
                block_frames: 128,
                channels: 2,
            };
            assert_eq!(config.validate(), Ok(()), "{rate}");
        }
    }

    #[test]
    fn errors_describe_themselves() {
        assert_eq!(
            AudioError::DeviceMissing.to_string(),
            "no such audio device"
        );
        assert_eq!(
            AudioError::Unsupported("block size").to_string(),
            "unsupported block size"
        );
        assert_eq!(
            AudioError::WrongState.to_string(),
            "the stream is in the wrong state"
        );
        assert_eq!(
            AudioError::Host("device busy").to_string(),
            "the audio host refused: device busy"
        );
    }
}
