//! Blocking client for an isolated CLAP processing process.
//!
//! This client performs pipe I/O and is intended for a dedicated IPC thread.
//! Audio callbacks must exchange blocks with that thread through bounded queues.

use super::clap::{
    MAX_NOTE_EVENTS, MAX_PARAMETER_EVENTS, MAX_STATE_BYTES, NoteEvent, ParameterEvent,
    ParameterInfo,
};
use std::ffi::OsStr;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const HEADER: [u8; 8] = *b"NYWORK4\0";
const SAVE_STATE: u32 = u32::MAX;
const LOAD_STATE: u32 = u32::MAX - 1;
pub const MAX_BLOCK_FRAMES: usize = 8_192;
const MAX_PARAMETERS: usize = 16_384;
const MAX_PARAMETER_NAME_BYTES: usize = 255;
const MAX_PARAMETER_MODULE_BYTES: usize = 1_023;
const STOP_TIMEOUT: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum Error {
    InvalidConfiguration,
    Spawn(io::Error),
    MissingPipe,
    Input(io::Error),
    Output(io::Error),
    InvalidProtocol,
    ParameterLimit,
    InvalidBlock,
    StateTooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => output.write_str("Worker configuration is invalid"),
            Self::Spawn(error) => write!(output, "Worker process could not start: {error}"),
            Self::MissingPipe => output.write_str("Worker process pipe is unavailable"),
            Self::Input(error) => write!(output, "Worker input failed: {error}"),
            Self::Output(error) => write!(output, "Worker output failed: {error}"),
            Self::InvalidProtocol => output.write_str("Worker protocol response is invalid"),
            Self::ParameterLimit => output.write_str("Worker parameter limit was exceeded"),
            Self::InvalidBlock => output.write_str("Worker audio block is invalid"),
            Self::StateTooLarge => output.write_str("Plugin state exceeds the size limit"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug)]
struct Handshake {
    latency_frames: u32,
    input_audio_ports: u32,
    input_note_ports: u32,
    parameters: Vec<ParameterInfo>,
}

/// Owns one plugin process and its blocking protocol pipes.
pub struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: ChildStdout,
    latency_frames: u32,
    input_audio_ports: u32,
    input_note_ports: u32,
    parameters: Vec<ParameterInfo>,
    max_frames: usize,
    request: Vec<u8>,
    response: Vec<u8>,
}

impl Client {
    pub fn spawn(
        executable: impl AsRef<OsStr>,
        package: impl AsRef<Path>,
        identifier: &str,
        sample_rate: f64,
        max_frames: usize,
    ) -> Result<Self, Error> {
        if identifier.is_empty()
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
            || !(1..=MAX_BLOCK_FRAMES).contains(&max_frames)
        {
            return Err(Error::InvalidConfiguration);
        }
        let mut child = Command::new(executable)
            .arg("clap")
            .arg(package.as_ref())
            .arg(identifier)
            .arg(sample_rate.to_string())
            .arg(max_frames.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(Error::Spawn)?;
        let Some(mut input) = child.stdin.take() else {
            terminate(&mut child);
            return Err(Error::MissingPipe);
        };
        let Some(mut output) = child.stdout.take() else {
            terminate(&mut child);
            return Err(Error::MissingPipe);
        };
        if let Err(error) = input.write_all(&HEADER).map_err(Error::Input) {
            terminate(&mut child);
            return Err(error);
        }
        if let Err(error) = input.flush().map_err(Error::Input) {
            terminate(&mut child);
            return Err(error);
        }
        let handshake = match read_handshake(&mut output) {
            Ok(handshake) => handshake,
            Err(error) => {
                terminate(&mut child);
                return Err(error);
            }
        };
        let request_capacity =
            12 + MAX_PARAMETER_EVENTS * 16 + MAX_NOTE_EVENTS * 26 + max_frames * 8;
        Ok(Self {
            child,
            input: Some(input),
            output,
            latency_frames: handshake.latency_frames,
            input_audio_ports: handshake.input_audio_ports,
            input_note_ports: handshake.input_note_ports,
            parameters: handshake.parameters,
            max_frames,
            request: Vec::with_capacity(request_capacity),
            response: vec![0; max_frames * 8],
        })
    }

    #[must_use]
    pub const fn latency_frames(&self) -> u32 {
        self.latency_frames
    }

    #[must_use]
    pub const fn input_audio_ports(&self) -> u32 {
        self.input_audio_ports
    }

    #[must_use]
    pub const fn input_note_ports(&self) -> u32 {
        self.input_note_ports
    }

    #[must_use]
    pub fn parameters(&self) -> &[ParameterInfo] {
        &self.parameters
    }

    pub fn process_stereo(
        &mut self,
        input: Option<(&[f32], &[f32])>,
        output_left: &mut [f32],
        output_right: &mut [f32],
        parameter_events: &[ParameterEvent],
        note_events: &[NoteEvent],
    ) -> Result<(), Error> {
        let frames = output_left.len();
        if frames == 0
            || frames != output_right.len()
            || frames > self.max_frames
            || parameter_events.len() > MAX_PARAMETER_EVENTS
            || note_events.len() > MAX_NOTE_EVENTS
            || input.is_some_and(|(left, right)| left.len() != frames || right.len() != frames)
            || parameter_events
                .iter()
                .any(|event| event.sample_offset as usize >= frames || !event.value.is_finite())
            || parameter_events
                .windows(2)
                .any(|events| events[0].sample_offset > events[1].sample_offset)
            || note_events.iter().any(|event| {
                event.sample_offset as usize >= frames
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

        self.request.clear();
        push_u32(&mut self.request, frames as u32);
        push_u32(&mut self.request, parameter_events.len() as u32);
        push_u32(&mut self.request, note_events.len() as u32);
        for event in parameter_events {
            push_u32(&mut self.request, event.sample_offset);
            push_u32(&mut self.request, event.identifier);
            self.request.extend_from_slice(&event.value.to_le_bytes());
        }
        for event in note_events {
            push_u32(&mut self.request, event.sample_offset);
            push_u32(&mut self.request, event.kind);
            self.request.extend_from_slice(&event.note_id.to_le_bytes());
            self.request
                .extend_from_slice(&event.port_index.to_le_bytes());
            self.request.extend_from_slice(&event.channel.to_le_bytes());
            self.request.extend_from_slice(&event.key.to_le_bytes());
            self.request
                .extend_from_slice(&event.velocity.to_le_bytes());
        }
        for frame in 0..frames {
            let [left, right] = match input {
                Some((left, right)) => [left[frame], right[frame]],
                None => [0.0, 0.0],
            };
            self.request.extend_from_slice(&left.to_le_bytes());
            self.request.extend_from_slice(&right.to_le_bytes());
        }
        self.write_request()?;

        let response_frames = read_u32(&mut self.output).map_err(Error::Output)? as usize;
        if response_frames != frames {
            return Err(Error::InvalidProtocol);
        }
        let byte_count = frames * 8;
        self.output
            .read_exact(&mut self.response[..byte_count])
            .map_err(Error::Output)?;
        for frame in 0..frames {
            let offset = frame * 8;
            output_left[frame] = f32::from_le_bytes(
                self.response[offset..offset + 4]
                    .try_into()
                    .map_err(|_| Error::InvalidProtocol)?,
            );
            output_right[frame] = f32::from_le_bytes(
                self.response[offset + 4..offset + 8]
                    .try_into()
                    .map_err(|_| Error::InvalidProtocol)?,
            );
        }
        Ok(())
    }

    pub fn save_state(&mut self) -> Result<Vec<u8>, Error> {
        self.request.clear();
        push_u32(&mut self.request, SAVE_STATE);
        self.write_request()?;
        if read_u32(&mut self.output).map_err(Error::Output)? != SAVE_STATE {
            return Err(Error::InvalidProtocol);
        }
        let length = usize::try_from(read_u64(&mut self.output).map_err(Error::Output)?)
            .ok()
            .filter(|length| *length <= MAX_STATE_BYTES)
            .ok_or(Error::StateTooLarge)?;
        let mut state = vec![0; length];
        self.output.read_exact(&mut state).map_err(Error::Output)?;
        Ok(state)
    }

    pub fn load_state(&mut self, state: &[u8]) -> Result<(), Error> {
        if state.len() > MAX_STATE_BYTES {
            return Err(Error::StateTooLarge);
        }
        self.request.clear();
        push_u32(&mut self.request, LOAD_STATE);
        self.request
            .extend_from_slice(&(state.len() as u64).to_le_bytes());
        self.request.extend_from_slice(state);
        self.write_request()?;
        if read_u32(&mut self.output).map_err(Error::Output)? != LOAD_STATE {
            return Err(Error::InvalidProtocol);
        }
        Ok(())
    }

    pub fn shutdown(mut self) {
        self.stop();
    }

    fn write_request(&mut self) -> Result<(), Error> {
        let input = self.input.as_mut().ok_or(Error::MissingPipe)?;
        input.write_all(&self.request).map_err(Error::Input)?;
        input.flush().map_err(Error::Input)
    }

    fn stop(&mut self) {
        if let Some(mut input) = self.input.take() {
            let _ = input.write_all(&0_u32.to_le_bytes());
            let _ = input.flush();
        }
        let deadline = Instant::now() + STOP_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(1)),
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

fn read_handshake(input: &mut impl Read) -> Result<Handshake, Error> {
    let mut header = [0; HEADER.len()];
    input.read_exact(&mut header).map_err(Error::Output)?;
    if header != HEADER {
        return Err(Error::InvalidProtocol);
    }
    let latency_frames = read_u32(input).map_err(Error::Output)?;
    let parameter_count = read_u32(input).map_err(Error::Output)? as usize;
    let input_audio_ports = read_u32(input).map_err(Error::Output)?;
    let input_note_ports = read_u32(input).map_err(Error::Output)?;
    if parameter_count > MAX_PARAMETERS {
        return Err(Error::ParameterLimit);
    }
    let mut parameters = Vec::with_capacity(parameter_count);
    for _ in 0..parameter_count {
        let identifier = read_u32(input).map_err(Error::Output)?;
        let flags = read_u32(input).map_err(Error::Output)?;
        let minimum = read_f64(input).map_err(Error::Output)?;
        let maximum = read_f64(input).map_err(Error::Output)?;
        let default_value = read_f64(input).map_err(Error::Output)?;
        let name = read_string(input, MAX_PARAMETER_NAME_BYTES)?;
        let module = read_string(input, MAX_PARAMETER_MODULE_BYTES)?;
        if !minimum.is_finite()
            || !maximum.is_finite()
            || !default_value.is_finite()
            || minimum > maximum
            || !(minimum..=maximum).contains(&default_value)
            || parameters
                .iter()
                .any(|parameter: &ParameterInfo| parameter.identifier == identifier)
        {
            return Err(Error::InvalidProtocol);
        }
        parameters.push(ParameterInfo {
            identifier,
            flags,
            name,
            module,
            minimum,
            maximum,
            default_value,
        });
    }
    Ok(Handshake {
        latency_frames,
        input_audio_ports,
        input_note_ports,
        parameters,
    })
}

fn read_string(input: &mut impl Read, limit: usize) -> Result<String, Error> {
    let length = read_u16(input).map_err(Error::Output)? as usize;
    if length > limit {
        return Err(Error::InvalidProtocol);
    }
    let mut bytes = vec![0; length];
    input.read_exact(&mut bytes).map_err(Error::Output)?;
    String::from_utf8(bytes).map_err(|_| Error::InvalidProtocol)
}

fn read_u16(input: &mut impl Read) -> io::Result<u16> {
    let mut bytes = [0; 2];
    input.read_exact(&mut bytes)?;
    Ok(u16::from_le_bytes(bytes))
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(input: &mut impl Read) -> io::Result<u64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_f64(input: &mut impl Read) -> io::Result<f64> {
    let mut bytes = [0; 8];
    input.read_exact(&mut bytes)?;
    Ok(f64::from_le_bytes(bytes))
}

fn push_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn handshake_bytes() -> Vec<u8> {
        let mut bytes = HEADER.to_vec();
        push_u32(&mut bytes, 64);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 2);
        push_u32(&mut bytes, 7);
        push_u32(&mut bytes, 32);
        bytes.extend_from_slice(&0.0_f64.to_le_bytes());
        bytes.extend_from_slice(&1.0_f64.to_le_bytes());
        bytes.extend_from_slice(&0.5_f64.to_le_bytes());
        bytes.extend_from_slice(&4_u16.to_le_bytes());
        bytes.extend_from_slice(b"Gain");
        bytes.extend_from_slice(&6_u16.to_le_bytes());
        bytes.extend_from_slice(b"Output");
        bytes
    }

    #[test]
    fn handshake_reads_bounded_metadata() {
        let handshake = read_handshake(&mut Cursor::new(handshake_bytes())).unwrap();
        assert_eq!(handshake.latency_frames, 64);
        assert_eq!(handshake.input_audio_ports, 1);
        assert_eq!(handshake.input_note_ports, 2);
        assert_eq!(
            handshake.parameters,
            vec![ParameterInfo {
                identifier: 7,
                flags: 32,
                name: "Gain".into(),
                module: "Output".into(),
                minimum: 0.0,
                maximum: 1.0,
                default_value: 0.5,
            }]
        );
    }

    #[test]
    fn handshake_rejects_header_and_metadata_errors() {
        let mut wrong_header = handshake_bytes();
        wrong_header[0] = b'X';
        assert!(matches!(
            read_handshake(&mut Cursor::new(wrong_header)),
            Err(Error::InvalidProtocol)
        ));

        let mut invalid_utf8 = handshake_bytes();
        let name = 8 + 16 + 32 + 2;
        invalid_utf8[name] = 0xff;
        assert!(matches!(
            read_handshake(&mut Cursor::new(invalid_utf8)),
            Err(Error::InvalidProtocol)
        ));

        let mut invalid_range = handshake_bytes();
        let minimum = 8 + 16 + 8;
        invalid_range[minimum..minimum + 8].copy_from_slice(&2.0_f64.to_le_bytes());
        assert!(matches!(
            read_handshake(&mut Cursor::new(invalid_range)),
            Err(Error::InvalidProtocol)
        ));
    }

    #[test]
    fn strings_are_length_limited_before_payload_reads() {
        let bytes = (256_u16).to_le_bytes();
        assert!(matches!(
            read_string(&mut Cursor::new(bytes), MAX_PARAMETER_NAME_BYTES),
            Err(Error::InvalidProtocol)
        ));
    }
}
