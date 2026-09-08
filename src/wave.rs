//! Reading and writing uncompressed audio files.
//!
//! This is the interchange format a bounce lands in and a sample is
//! loaded from. Only the parts a workstation needs are handled: the
//! standard chunk layout, integer samples of 8, 16, 24, and 32 bits, and
//! floating-point samples of 32 and 64 bits, in any channel count.
//!
//! Both directions work on whole files rather than streaming, because a
//! bounce is written from a finished render and a sample is loaded before
//! playback. Neither runs on the audio thread.

use std::io::{self, Read, Seek, SeekFrom, Write};

/// How samples are stored in a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleFormat {
    /// Signed integers, except 8-bit which is unsigned by convention.
    Integer,
    /// Floating point.
    Float,
}

/// What a file contains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Format {
    /// Frames per second.
    pub sample_rate: u32,
    /// Channels per frame.
    pub channels: u16,
    /// Bits in one sample.
    pub bits: u16,
    /// How the samples are stored.
    pub sample_format: SampleFormat,
}

impl Format {
    /// The format a bounce is written in by default: stereo, 24-bit,
    /// which is what a mastering chain expects.
    #[must_use]
    pub const fn stereo(sample_rate: u32) -> Self {
        Self {
            sample_rate,
            channels: 2,
            bits: 24,
            sample_format: SampleFormat::Integer,
        }
    }

    /// Bytes in one frame.
    #[must_use]
    pub const fn bytes_per_frame(&self) -> usize {
        (self.bits as usize / 8) * self.channels as usize
    }

    /// Whether this combination can be written.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        if self.sample_rate == 0 || self.channels == 0 {
            return false;
        }
        match self.sample_format {
            SampleFormat::Integer => matches!(self.bits, 8 | 16 | 24 | 32),
            SampleFormat::Float => matches!(self.bits, 32 | 64),
        }
    }
}

/// Why a file could not be read or written.
#[derive(Debug)]
pub enum WaveError {
    /// The file could not be reached.
    Io(io::Error),
    /// The file is not of the expected kind.
    NotAWaveFile,
    /// A required chunk is absent.
    MissingChunk(&'static str),
    /// The header describes something this cannot handle.
    Unsupported(&'static str),
    /// The file ends in the middle of what it declared.
    Truncated,
}

impl From<io::Error> for WaveError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl core::fmt::Display for WaveError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "{error}"),
            Self::NotAWaveFile => formatter.write_str("not a wave file"),
            Self::MissingChunk(name) => write!(formatter, "the {name} chunk is missing"),
            Self::Unsupported(what) => write!(formatter, "unsupported {what}"),
            Self::Truncated => formatter.write_str("the file ends early"),
        }
    }
}

impl std::error::Error for WaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

/// Writes samples to a file as they are produced.
///
/// The header records a length that is only known at the end, so the
/// writer goes back and fills it in when it is finished. Dropping a
/// writer without finishing leaves a file whose header understates its
/// length; call [`finish`](Self::finish) to close it properly.
pub struct WaveWriter<W: Write + Seek> {
    // Held as an option so the destination can be handed back out of
    // `finish` even though this type also cleans up when dropped.
    inner: Option<W>,
    format: Format,
    frames_written: u64,
    finished: bool,
}

impl<W: Write + Seek> WaveWriter<W> {
    /// Starts a file, writing its header.
    ///
    /// # Errors
    ///
    /// Returns [`WaveError::Unsupported`] for a format that cannot be
    /// written, or an I/O error from the destination.
    pub fn new(mut inner: W, format: Format) -> Result<Self, WaveError> {
        if !format.is_supported() {
            return Err(WaveError::Unsupported("sample format"));
        }
        write_header(&mut inner, format, 0)?;
        Ok(Self {
            inner: Some(inner),
            format,
            frames_written: 0,
            finished: false,
        })
    }

    /// The format being written.
    #[inline]
    #[must_use]
    pub const fn format(&self) -> Format {
        self.format
    }

    /// Frames written so far.
    #[inline]
    #[must_use]
    pub const fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Writes interleaved samples. The count must be a whole number of
    /// frames.
    ///
    /// Values outside -1 to 1 are clamped for an integer format, since
    /// wrapping a sample turns an overshoot into a click.
    ///
    /// # Errors
    ///
    /// Returns [`WaveError::Unsupported`] when the count is not a whole
    /// number of frames, or an I/O error.
    pub fn write_samples(&mut self, samples: &[f32]) -> Result<(), WaveError> {
        let channels = self.format.channels as usize;
        if samples.len() % channels != 0 {
            return Err(WaveError::Unsupported("partial frame"));
        }
        let mut bytes = Vec::with_capacity(samples.len() * (self.format.bits as usize / 8));
        for sample in samples {
            encode_sample(*sample, self.format, &mut bytes);
        }
        self.destination().write_all(&bytes)?;
        self.frames_written += (samples.len() / channels) as u64;
        Ok(())
    }

    /// Writes stereo frames. Only valid for a two-channel format.
    ///
    /// # Errors
    ///
    /// Returns [`WaveError::Unsupported`] when the format is not stereo,
    /// or an I/O error.
    pub fn write_stereo(&mut self, frames: &[[f32; 2]]) -> Result<(), WaveError> {
        if self.format.channels != 2 {
            return Err(WaveError::Unsupported("channel count"));
        }
        let mut bytes = Vec::with_capacity(frames.len() * self.format.bytes_per_frame());
        for frame in frames {
            encode_sample(frame[0], self.format, &mut bytes);
            encode_sample(frame[1], self.format, &mut bytes);
        }
        self.destination().write_all(&bytes)?;
        self.frames_written += frames.len() as u64;
        Ok(())
    }

    /// Completes the file, filling in the lengths, and returns the
    /// destination.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from the destination.
    pub fn finish(mut self) -> Result<W, WaveError> {
        self.finalize()?;
        self.inner.take().ok_or(WaveError::Truncated)
    }

    // The destination, which is present for the whole life of a writer
    // apart from the moment `finish` hands it back.
    fn destination(&mut self) -> &mut W {
        self.inner
            .as_mut()
            .unwrap_or_else(|| unreachable!("the destination was taken"))
    }

    fn finalize(&mut self) -> Result<(), WaveError> {
        if self.finished || self.inner.is_none() {
            return Ok(());
        }
        self.finished = true;
        let data_bytes = self.frames_written * self.format.bytes_per_frame() as u64;
        // The data chunk is padded to an even length.
        let padding = (data_bytes % 2) as usize;
        if padding == 1 {
            self.destination().write_all(&[0])?;
        }
        let position = self.destination().stream_position()?;
        // The overall length counts everything after the first eight bytes.
        self.destination().seek(SeekFrom::Start(4))?;
        let total = u32::try_from(position.saturating_sub(8)).unwrap_or(u32::MAX);
        self.destination().write_all(&total.to_le_bytes())?;
        // The data chunk's length sits just before the samples.
        self.destination()
            .seek(SeekFrom::Start(HEADER_BYTES as u64 - 4))?;
        let declared = u32::try_from(data_bytes).unwrap_or(u32::MAX);
        self.destination().write_all(&declared.to_le_bytes())?;
        self.destination().seek(SeekFrom::Start(position))?;
        self.destination().flush()?;
        Ok(())
    }
}

impl<W: Write + Seek> Drop for WaveWriter<W> {
    fn drop(&mut self) {
        // A writer dropped without finishing still gets a usable header,
        // so a file left behind by an interrupted bounce can be opened.
        let _ = self.finalize();
    }
}

/// Bytes before the samples: the outer header, the format chunk, and the
/// data chunk's own header.
const HEADER_BYTES: usize = 12 + 24 + 8;

fn write_header<W: Write>(inner: &mut W, format: Format, data_bytes: u32) -> Result<(), WaveError> {
    let mut header = Vec::with_capacity(HEADER_BYTES);
    header.extend_from_slice(b"RIFF");
    header.extend_from_slice(&(HEADER_BYTES as u32 - 8 + data_bytes).to_le_bytes());
    header.extend_from_slice(b"WAVE");

    header.extend_from_slice(b"fmt ");
    header.extend_from_slice(&16_u32.to_le_bytes());
    let tag = match format.sample_format {
        SampleFormat::Integer => FORMAT_PCM,
        SampleFormat::Float => FORMAT_FLOAT,
    };
    header.extend_from_slice(&tag.to_le_bytes());
    header.extend_from_slice(&format.channels.to_le_bytes());
    header.extend_from_slice(&format.sample_rate.to_le_bytes());
    let bytes_per_second = format.sample_rate * format.bytes_per_frame() as u32;
    header.extend_from_slice(&bytes_per_second.to_le_bytes());
    header.extend_from_slice(&(format.bytes_per_frame() as u16).to_le_bytes());
    header.extend_from_slice(&format.bits.to_le_bytes());

    header.extend_from_slice(b"data");
    header.extend_from_slice(&data_bytes.to_le_bytes());
    inner.write_all(&header)?;
    Ok(())
}

fn encode_sample(sample: f32, format: Format, out: &mut Vec<u8>) {
    match (format.sample_format, format.bits) {
        (SampleFormat::Float, 32) => out.extend_from_slice(&sample.to_le_bytes()),
        (SampleFormat::Float, _) => out.extend_from_slice(&f64::from(sample).to_le_bytes()),
        (SampleFormat::Integer, bits) => {
            let clamped = if sample.is_nan() {
                0.0
            } else {
                sample.clamp(-1.0, 1.0)
            };
            match bits {
                8 => {
                    // Eight-bit samples are unsigned, centred on 128.
                    let value = ((clamped * 127.0) + 128.0).round().clamp(0.0, 255.0) as u8;
                    out.push(value);
                }
                16 => {
                    let value = (clamped * f32::from(i16::MAX)).round() as i16;
                    out.extend_from_slice(&value.to_le_bytes());
                }
                24 => {
                    let value = (f64::from(clamped) * 8_388_607.0).round() as i32;
                    let bytes = value.to_le_bytes();
                    out.extend_from_slice(&bytes[..3]);
                }
                _ => {
                    let value = (f64::from(clamped) * f64::from(i32::MAX)).round() as i32;
                    out.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
}

/// A file read into memory.
#[derive(Clone, Debug, PartialEq)]
pub struct WaveFile {
    /// What the file contained.
    pub format: Format,
    /// Interleaved samples, scaled to -1 through 1.
    pub samples: Vec<f32>,
}

impl WaveFile {
    /// Frames in the file.
    #[must_use]
    pub fn frames(&self) -> usize {
        if self.format.channels == 0 {
            0
        } else {
            self.samples.len() / self.format.channels as usize
        }
    }

    /// Seconds the file lasts.
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.format.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f64 / f64::from(self.format.sample_rate)
        }
    }

    /// One frame as stereo, duplicating a mono file's single channel and
    /// taking the first two channels of anything wider.
    #[must_use]
    pub fn stereo_frame(&self, index: usize) -> [f32; 2] {
        let channels = self.format.channels as usize;
        if channels == 0 || index >= self.frames() {
            return [0.0, 0.0];
        }
        let base = index * channels;
        if channels == 1 {
            [self.samples[base], self.samples[base]]
        } else {
            [self.samples[base], self.samples[base + 1]]
        }
    }
}

/// Reads a whole file.
///
/// Chunks other than the format and data chunks are skipped, so a file
/// carrying metadata still loads.
///
/// # Errors
///
/// Returns [`WaveError`] when the file is not readable, is not of the
/// expected kind, or describes a format this cannot decode.
pub fn read<R: Read + Seek>(mut inner: R) -> Result<WaveFile, WaveError> {
    // The declared chunk sizes are checked against the real length, so a
    // corrupt header cannot make this reserve memory for bytes that are
    // not there.
    let start = inner.stream_position()?;
    let length = inner.seek(SeekFrom::End(0))?;
    inner.seek(SeekFrom::Start(start))?;

    let mut riff = [0_u8; 12];
    inner.read_exact(&mut riff).map_err(|error| {
        if error.kind() == io::ErrorKind::UnexpectedEof {
            WaveError::NotAWaveFile
        } else {
            WaveError::Io(error)
        }
    })?;
    if &riff[..4] != b"RIFF" || &riff[8..] != b"WAVE" {
        return Err(WaveError::NotAWaveFile);
    }

    let mut format: Option<Format> = None;
    loop {
        let mut header = [0_u8; 8];
        match inner.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(WaveError::Io(error)),
        }
        let id = [header[0], header[1], header[2], header[3]];
        let size = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as u64;
        let position = inner.stream_position()?;
        if size > length.saturating_sub(position) {
            return Err(WaveError::Truncated);
        }
        // Chunks are padded to an even length.
        let padded = size + (size % 2);

        if &id == b"fmt " {
            if size < 16 {
                return Err(WaveError::Truncated);
            }
            let mut chunk = vec![0_u8; size as usize];
            inner
                .read_exact(&mut chunk)
                .map_err(|_| WaveError::Truncated)?;
            let mut tag = u16::from_le_bytes([chunk[0], chunk[1]]);
            let channels = u16::from_le_bytes([chunk[2], chunk[3]]);
            let sample_rate = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            let bits = u16::from_le_bytes([chunk[14], chunk[15]]);
            if tag == FORMAT_EXTENSIBLE {
                // The real tag sits at the start of the extension.
                if chunk.len() < 26 {
                    return Err(WaveError::Truncated);
                }
                tag = u16::from_le_bytes([chunk[24], chunk[25]]);
            }
            let sample_format = match tag {
                FORMAT_PCM => SampleFormat::Integer,
                FORMAT_FLOAT => SampleFormat::Float,
                _ => return Err(WaveError::Unsupported("compression")),
            };
            let described = Format {
                sample_rate,
                channels,
                bits,
                sample_format,
            };
            if !described.is_supported() {
                return Err(WaveError::Unsupported("sample format"));
            }
            format = Some(described);
            let skip = padded - size;
            if skip > 0 {
                inner.seek(SeekFrom::Current(skip as i64))?;
            }
        } else if &id == b"data" {
            let Some(format) = format else {
                return Err(WaveError::MissingChunk("fmt "));
            };
            let mut bytes = vec![0_u8; size as usize];
            inner
                .read_exact(&mut bytes)
                .map_err(|_| WaveError::Truncated)?;
            let samples = decode_samples(&bytes, format);
            return Ok(WaveFile { format, samples });
        } else {
            inner.seek(SeekFrom::Current(padded as i64))?;
        }
    }
    Err(WaveError::MissingChunk("data"))
}

fn decode_samples(bytes: &[u8], format: Format) -> Vec<f32> {
    let width = format.bits as usize / 8;
    if width == 0 {
        return Vec::new();
    }
    let count = bytes.len() / width;
    let mut samples = Vec::with_capacity(count);
    for index in 0..count {
        let at = index * width;
        let value = match (format.sample_format, format.bits) {
            (SampleFormat::Float, 32) => {
                f32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
            }
            (SampleFormat::Float, _) => f64::from_le_bytes([
                bytes[at],
                bytes[at + 1],
                bytes[at + 2],
                bytes[at + 3],
                bytes[at + 4],
                bytes[at + 5],
                bytes[at + 6],
                bytes[at + 7],
            ]) as f32,
            (SampleFormat::Integer, 8) => (f32::from(bytes[at]) - 128.0) / 127.0,
            (SampleFormat::Integer, 16) => {
                f32::from(i16::from_le_bytes([bytes[at], bytes[at + 1]])) / 32_767.0
            }
            (SampleFormat::Integer, 24) => {
                // Sign-extend the three bytes into a full integer.
                let raw = i32::from_le_bytes([0, bytes[at], bytes[at + 1], bytes[at + 2]]) >> 8;
                raw as f32 / 8_388_607.0
            }
            (SampleFormat::Integer, _) => {
                let raw =
                    i32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
                (f64::from(raw) / f64::from(i32::MAX)) as f32
            }
        };
        samples.push(value);
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn round_trip(format: Format, frames: &[[f32; 2]]) -> WaveFile {
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), format).unwrap();
        writer.write_stereo(frames).unwrap();
        let cursor = writer.finish().unwrap();
        read(Cursor::new(cursor.into_inner())).unwrap()
    }

    fn tone(frames: usize) -> Vec<[f32; 2]> {
        (0..frames)
            .map(|index| {
                let phase = index as f32 * 0.05;
                [phase.sin() * 0.5, (phase * 1.5).cos() * 0.25]
            })
            .collect()
    }

    #[test]
    fn a_written_file_reads_back() {
        let frames = tone(1_000);
        let file = round_trip(Format::stereo(48_000), &frames);
        assert_eq!(file.format.sample_rate, 48_000);
        assert_eq!(file.format.channels, 2);
        assert_eq!(file.format.bits, 24);
        assert_eq!(file.frames(), 1_000);
        assert!((file.seconds() - 1_000.0 / 48_000.0).abs() < 1e-9);
        for (index, frame) in frames.iter().enumerate() {
            let read = file.stereo_frame(index);
            // Twenty-four bits resolve to better than a millionth.
            assert!((read[0] - frame[0]).abs() < 1e-5, "{index}");
            assert!((read[1] - frame[1]).abs() < 1e-5, "{index}");
        }
    }

    #[test]
    fn every_supported_depth_round_trips_within_its_resolution() {
        let frames = tone(500);
        let cases = [
            (8_u16, SampleFormat::Integer, 0.02_f32),
            (16, SampleFormat::Integer, 1e-4),
            (24, SampleFormat::Integer, 1e-5),
            (32, SampleFormat::Integer, 1e-6),
            (32, SampleFormat::Float, 1e-9),
            (64, SampleFormat::Float, 1e-9),
        ];
        for (bits, sample_format, tolerance) in cases {
            let format = Format {
                sample_rate: 44_100,
                channels: 2,
                bits,
                sample_format,
            };
            let file = round_trip(format, &frames);
            assert_eq!(file.format, format, "{bits} {sample_format:?}");
            for (index, frame) in frames.iter().enumerate() {
                let read = file.stereo_frame(index);
                assert!(
                    (read[0] - frame[0]).abs() < tolerance,
                    "{bits} {sample_format:?} at {index}: {} vs {}",
                    read[0],
                    frame[0]
                );
            }
        }
    }

    #[test]
    fn samples_beyond_full_scale_are_clamped_rather_than_wrapped() {
        let frames = [[2.0_f32, -2.0], [1.5, -1.5], [f32::NAN, 0.0]];
        let file = round_trip(Format::stereo(48_000), &frames);
        for index in 0..2 {
            let read = file.stereo_frame(index);
            assert!(read[0] > 0.99, "{index}: {}", read[0]);
            assert!(read[1] < -0.99, "{index}: {}", read[1]);
        }
        // A value that is not a number becomes silence, not noise.
        assert_eq!(file.stereo_frame(2)[0], 0.0);
    }

    #[test]
    fn an_empty_file_is_valid() {
        let file = round_trip(Format::stereo(48_000), &[]);
        assert_eq!(file.frames(), 0);
        assert_eq!(file.seconds(), 0.0);
        assert_eq!(file.stereo_frame(0), [0.0, 0.0]);
    }

    #[test]
    fn an_odd_number_of_bytes_is_padded() {
        // One frame of 8-bit mono is a single byte, so the chunk is odd.
        let format = Format {
            sample_rate: 8_000,
            channels: 1,
            bits: 8,
            sample_format: SampleFormat::Integer,
        };
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), format).unwrap();
        writer.write_samples(&[0.5]).unwrap();
        let cursor = writer.finish().unwrap();
        let bytes = cursor.into_inner();
        assert_eq!(bytes.len() % 2, 0, "the file was left an odd length");
        let file = read(Cursor::new(bytes)).unwrap();
        assert_eq!(file.frames(), 1);
    }

    #[test]
    fn a_mono_file_reads_as_stereo() {
        let format = Format {
            sample_rate: 48_000,
            channels: 1,
            bits: 16,
            sample_format: SampleFormat::Integer,
        };
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), format).unwrap();
        writer.write_samples(&[0.25, -0.25, 0.5]).unwrap();
        let cursor = writer.finish().unwrap();
        let file = read(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(file.frames(), 3);
        let frame = file.stereo_frame(0);
        assert!((frame[0] - 0.25).abs() < 1e-4);
        assert_eq!(frame[0], frame[1], "mono is duplicated across the pair");
    }

    #[test]
    fn a_partial_frame_is_refused() {
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), Format::stereo(48_000)).unwrap();
        // Three samples do not divide into stereo frames.
        assert!(matches!(
            writer.write_samples(&[0.0, 0.0, 0.0]),
            Err(WaveError::Unsupported("partial frame"))
        ));
        assert_eq!(writer.frames_written(), 0);
    }

    #[test]
    fn writing_stereo_to_a_mono_file_is_refused() {
        let format = Format {
            sample_rate: 48_000,
            channels: 1,
            bits: 16,
            sample_format: SampleFormat::Integer,
        };
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), format).unwrap();
        assert!(matches!(
            writer.write_stereo(&[[0.0, 0.0]]),
            Err(WaveError::Unsupported("channel count"))
        ));
    }

    #[test]
    fn an_unsupported_format_is_refused_before_anything_is_written() {
        for format in [
            Format {
                sample_rate: 48_000,
                channels: 2,
                bits: 12,
                sample_format: SampleFormat::Integer,
            },
            Format {
                sample_rate: 48_000,
                channels: 2,
                bits: 16,
                sample_format: SampleFormat::Float,
            },
            Format {
                sample_rate: 0,
                channels: 2,
                bits: 16,
                sample_format: SampleFormat::Integer,
            },
            Format {
                sample_rate: 48_000,
                channels: 0,
                bits: 16,
                sample_format: SampleFormat::Integer,
            },
        ] {
            assert!(!format.is_supported(), "{format:?}");
            let result = WaveWriter::new(Cursor::new(Vec::new()), format);
            assert!(result.is_err(), "{format:?}");
        }
    }

    #[test]
    fn a_file_that_is_not_a_wave_file_is_refused() {
        assert!(matches!(
            read(Cursor::new(b"not audio at all".to_vec())),
            Err(WaveError::NotAWaveFile)
        ));
        assert!(matches!(
            read(Cursor::new(Vec::new())),
            Err(WaveError::NotAWaveFile)
        ));
        // The right length, the wrong contents.
        let mut wrong = b"RIFF".to_vec();
        wrong.extend_from_slice(&100_u32.to_le_bytes());
        wrong.extend_from_slice(b"AVI ");
        assert!(matches!(
            read(Cursor::new(wrong)),
            Err(WaveError::NotAWaveFile)
        ));
    }

    #[test]
    fn a_file_with_no_samples_chunk_is_refused() {
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&36_u32.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&FORMAT_PCM.to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&48_000_u32.to_le_bytes());
        bytes.extend_from_slice(&192_000_u32.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        assert!(matches!(
            read(Cursor::new(bytes)),
            Err(WaveError::MissingChunk("data"))
        ));
    }

    #[test]
    fn chunks_that_are_not_needed_are_skipped() {
        // A file with metadata before the samples still loads.
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), Format::stereo(48_000)).unwrap();
        writer.write_stereo(&[[0.5, -0.5]]).unwrap();
        let complete = writer.finish().unwrap().into_inner();

        let mut spliced = complete[..36].to_vec();
        spliced.extend_from_slice(b"LIST");
        spliced.extend_from_slice(&6_u32.to_le_bytes());
        spliced.extend_from_slice(b"INFOxx");
        spliced.extend_from_slice(&complete[36..]);
        let file = read(Cursor::new(spliced)).unwrap();
        assert_eq!(file.frames(), 1);
        assert!((file.stereo_frame(0)[0] - 0.5).abs() < 1e-5);
    }

    #[test]
    fn a_truncated_file_is_reported_rather_than_read_short() {
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), Format::stereo(48_000)).unwrap();
        writer.write_stereo(&tone(100)).unwrap();
        let complete = writer.finish().unwrap().into_inner();
        let cut = complete[..complete.len() - 60].to_vec();
        assert!(matches!(read(Cursor::new(cut)), Err(WaveError::Truncated)));
    }

    #[test]
    fn a_chunk_claiming_more_than_the_file_holds_is_refused() {
        // A header asking for four gibibytes must not make the reader
        // reserve four gibibytes.
        let mut bytes = b"RIFF".to_vec();
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            read(Cursor::new(bytes)),
            Err(WaveError::Truncated)
        ));
    }

    #[test]
    fn a_writer_dropped_without_finishing_still_leaves_a_readable_file() {
        let mut cursor = Cursor::new(Vec::new());
        {
            let mut writer = WaveWriter::new(&mut cursor, Format::stereo(48_000)).unwrap();
            writer.write_stereo(&tone(50)).unwrap();
            // Dropped here rather than finished.
        }
        let file = read(Cursor::new(cursor.into_inner())).unwrap();
        assert_eq!(file.frames(), 50);
    }

    #[test]
    fn the_declared_lengths_match_what_was_written() {
        let frames = tone(700);
        let mut writer = WaveWriter::new(Cursor::new(Vec::new()), Format::stereo(48_000)).unwrap();
        writer.write_stereo(&frames).unwrap();
        assert_eq!(writer.frames_written(), 700);
        let bytes = writer.finish().unwrap().into_inner();

        let declared_total = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        assert_eq!(declared_total as usize, bytes.len() - 8);
        let data_length = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]);
        assert_eq!(data_length as usize, 700 * 6);
    }

    #[test]
    fn errors_describe_themselves() {
        assert_eq!(WaveError::NotAWaveFile.to_string(), "not a wave file");
        assert_eq!(
            WaveError::MissingChunk("data").to_string(),
            "the data chunk is missing"
        );
        assert_eq!(
            WaveError::Unsupported("compression").to_string(),
            "unsupported compression"
        );
        assert_eq!(WaveError::Truncated.to_string(), "the file ends early");
    }
}
