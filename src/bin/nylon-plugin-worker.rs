use nylon::plugin::clap::{Instance, MAX_PARAMETER_EVENTS, MAX_STATE_BYTES, ParameterEvent};
use nylon::wave::{Format, SampleFormat, WaveWriter, read};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const HEADER: [u8; 8] = *b"NYWORK3\0";
const MAX_BLOCK_FRAMES: usize = 8_192;
const SAVE_STATE: u32 = u32::MAX;
const LOAD_STATE: u32 = u32::MAX - 1;

fn main() {
    let arguments = std::env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|value| value == "render-clap")
    {
        if arguments.len() != 6 {
            fail(
                "Usage: nylon-plugin-worker render-clap <plugin> <id> <input> <output> <block-frames>",
            );
        }
        let Some(identifier) = arguments[2].to_str() else {
            fail("Plugin identifier is invalid");
        };
        let block_frames = block_frames(&arguments[5]);
        render_file(
            Path::new(&arguments[1]),
            identifier,
            Path::new(&arguments[3]),
            Path::new(&arguments[4]),
            block_frames,
        )
        .unwrap_or_else(|error| fail(&error));
        return;
    }
    if arguments.len() != 5 || arguments.first().is_none_or(|value| value != "clap") {
        fail("Usage: nylon-plugin-worker clap <plugin> <id> <sample-rate> <max-frames>");
    }
    let Some(identifier) = arguments[2].to_str() else {
        fail("Plugin identifier is invalid");
    };
    let sample_rate = arguments[3]
        .to_str()
        .and_then(|value| value.parse::<f64>().ok())
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or_else(|| fail("Sample rate is invalid"));
    let max_frames = block_frames(&arguments[4]);
    let mut instance = Instance::open(Path::new(&arguments[1]), identifier)
        .unwrap_or_else(|error| fail(&error.to_string()));
    instance
        .activate(sample_rate, 1, max_frames as u32)
        .unwrap_or_else(|error| fail(&error.to_string()));
    run(&mut instance, max_frames).unwrap_or_else(|error| fail(error));
}

fn block_frames(value: &std::ffi::OsStr) -> usize {
    value
        .to_str()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| (1..=MAX_BLOCK_FRAMES).contains(value))
        .unwrap_or_else(|| fail("Block limit is invalid"))
}

fn render_file(
    package: &Path,
    identifier: &str,
    input_path: &Path,
    output_path: &Path,
    block_frames: usize,
) -> Result<(), String> {
    if output_path.exists() {
        return Err("Output file already exists".into());
    }
    let source = read(File::open(input_path).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    let mut instance = Instance::open(package, identifier).map_err(|error| error.to_string())?;
    instance
        .activate(source.format.sample_rate as f64, 1, block_frames as u32)
        .map_err(|error| error.to_string())?;

    let temporary_path = partial_path(output_path);
    let destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary_path)
        .map_err(|error| error.to_string())?;
    let mut cleanup = PartialOutput::new(temporary_path);
    let format = Format {
        sample_rate: source.format.sample_rate,
        channels: 2,
        bits: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WaveWriter::new(destination, format).map_err(|error| error.to_string())?;
    let mut input_left = vec![0.0; block_frames];
    let mut input_right = vec![0.0; block_frames];
    let mut output_left = vec![0.0; block_frames];
    let mut output_right = vec![0.0; block_frames];
    let mut output = vec![[0.0; 2]; block_frames];
    let mut position = 0;
    while position < source.frames() {
        let frames = block_frames.min(source.frames() - position);
        for frame in 0..frames {
            [input_left[frame], input_right[frame]] = source.stereo_frame(position + frame);
        }
        instance
            .process_stereo(
                Some((&input_left[..frames], &input_right[..frames])),
                &mut output_left[..frames],
                &mut output_right[..frames],
            )
            .map_err(|error| error.to_string())?;
        for frame in 0..frames {
            output[frame] = [output_left[frame], output_right[frame]];
        }
        writer
            .write_stereo(&output[..frames])
            .map_err(|error| error.to_string())?;
        position += frames;
    }
    writer.finish().map_err(|error| error.to_string())?;
    fs::rename(cleanup.path(), output_path).map_err(|error| error.to_string())?;
    cleanup.commit();
    Ok(())
}

fn partial_path(output: &Path) -> PathBuf {
    let mut path = output.as_os_str().to_owned();
    path.push(".partial");
    path.into()
}

struct PartialOutput {
    path: PathBuf,
    committed: bool,
}

impl PartialOutput {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for PartialOutput {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn run(instance: &mut Instance, max_frames: usize) -> Result<(), &'static str> {
    let mut input = io::stdin().lock();
    let mut output = io::stdout().lock();
    let mut header = [0; HEADER.len()];
    input
        .read_exact(&mut header)
        .map_err(|_| "Worker header is missing")?;
    if header != HEADER {
        return Err("Worker header is invalid");
    }
    output
        .write_all(&HEADER)
        .map_err(|_| "Worker output failed")?;
    output
        .write_all(
            &instance
                .latency_frames()
                .map_err(|_| "Plugin latency is unavailable")?
                .to_le_bytes(),
        )
        .map_err(|_| "Worker output failed")?;
    let parameters = instance
        .parameters()
        .map_err(|_| "Plugin parameters are unavailable")?;
    output
        .write_all(&(parameters.len() as u32).to_le_bytes())
        .map_err(|_| "Worker output failed")?;
    for parameter in parameters {
        let name = parameter.name.as_bytes();
        let module = parameter.module.as_bytes();
        let name_length = u16::try_from(name.len()).map_err(|_| "Parameter name is too long")?;
        let module_length =
            u16::try_from(module.len()).map_err(|_| "Parameter module is too long")?;
        output
            .write_all(&parameter.identifier.to_le_bytes())
            .and_then(|()| output.write_all(&parameter.flags.to_le_bytes()))
            .and_then(|()| output.write_all(&parameter.minimum.to_le_bytes()))
            .and_then(|()| output.write_all(&parameter.maximum.to_le_bytes()))
            .and_then(|()| output.write_all(&parameter.default_value.to_le_bytes()))
            .and_then(|()| output.write_all(&name_length.to_le_bytes()))
            .and_then(|()| output.write_all(name))
            .and_then(|()| output.write_all(&module_length.to_le_bytes()))
            .and_then(|()| output.write_all(module))
            .map_err(|_| "Worker output failed")?;
    }
    output.flush().map_err(|_| "Worker output failed")?;

    let mut bytes = vec![0; max_frames * 8];
    let mut input_left = vec![0.0; max_frames];
    let mut input_right = vec![0.0; max_frames];
    let mut output_left = vec![0.0; max_frames];
    let mut output_right = vec![0.0; max_frames];
    let mut parameter_events = Vec::with_capacity(MAX_PARAMETER_EVENTS);
    loop {
        let mut count = [0; 4];
        input
            .read_exact(&mut count)
            .map_err(|_| "Block header is missing")?;
        let frames = u32::from_le_bytes(count) as usize;
        if frames == 0 {
            return Ok(());
        }
        if frames == SAVE_STATE as usize {
            let state = instance
                .save_state()
                .map_err(|_| "Plugin state save failed")?;
            output
                .write_all(&SAVE_STATE.to_le_bytes())
                .and_then(|()| output.write_all(&(state.len() as u64).to_le_bytes()))
                .and_then(|()| output.write_all(&state))
                .and_then(|()| output.flush())
                .map_err(|_| "Worker output failed")?;
            continue;
        }
        if frames == LOAD_STATE as usize {
            let mut length = [0; 8];
            input
                .read_exact(&mut length)
                .map_err(|_| "Plugin state length is missing")?;
            let length = usize::try_from(u64::from_le_bytes(length))
                .ok()
                .filter(|length| *length <= MAX_STATE_BYTES)
                .ok_or("Plugin state exceeds the size limit")?;
            let mut state = vec![0; length];
            input
                .read_exact(&mut state)
                .map_err(|_| "Plugin state is truncated")?;
            instance
                .load_state(&state)
                .map_err(|_| "Plugin state load failed")?;
            output
                .write_all(&LOAD_STATE.to_le_bytes())
                .and_then(|()| output.flush())
                .map_err(|_| "Worker output failed")?;
            continue;
        }
        if frames > max_frames {
            return Err("Audio block exceeds the configured limit");
        }
        let mut event_count = [0; 4];
        input
            .read_exact(&mut event_count)
            .map_err(|_| "Parameter event count is missing")?;
        let event_count = u32::from_le_bytes(event_count) as usize;
        if event_count > MAX_PARAMETER_EVENTS {
            return Err("Parameter event count exceeds the configured limit");
        }
        parameter_events.clear();
        for _ in 0..event_count {
            let mut event = [0; 16];
            input
                .read_exact(&mut event)
                .map_err(|_| "Parameter event is truncated")?;
            parameter_events.push(ParameterEvent {
                sample_offset: u32::from_le_bytes(event[0..4].try_into().unwrap()),
                identifier: u32::from_le_bytes(event[4..8].try_into().unwrap()),
                value: f64::from_le_bytes(event[8..16].try_into().unwrap()),
            });
        }
        let byte_count = frames.checked_mul(8).ok_or("Audio block is too large")?;
        input
            .read_exact(&mut bytes[..byte_count])
            .map_err(|_| "Audio block is truncated")?;
        for frame in 0..frames {
            let offset = frame * 8;
            input_left[frame] = f32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
            input_right[frame] =
                f32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        }
        instance
            .process_stereo_with_events(
                Some((&input_left[..frames], &input_right[..frames])),
                &mut output_left[..frames],
                &mut output_right[..frames],
                &parameter_events,
            )
            .map_err(|_| "Plugin processing failed")?;
        for frame in 0..frames {
            let offset = frame * 8;
            bytes[offset..offset + 4].copy_from_slice(&output_left[frame].to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&output_right[frame].to_le_bytes());
        }
        output
            .write_all(&count)
            .and_then(|()| output.write_all(&bytes[..byte_count]))
            .and_then(|()| output.flush())
            .map_err(|_| "Worker output failed")?;
    }
}

fn fail(message: &str) -> ! {
    let _ = writeln!(io::stderr().lock(), "{message}");
    std::process::exit(1)
}
