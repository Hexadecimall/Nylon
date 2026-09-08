//! Fixed-capacity, sample-accurate stereo rendering.

pub mod playback;
pub mod sample;
pub mod schedule;
pub mod voice;

/// Maximum supported callback size in frames.
pub const MAX_FRAMES: usize = 2048;
/// Maximum parameter changes per callback.
pub const MAX_EVENTS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GainEvent {
    pub offset: usize,
    pub gain: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError {
    BufferSize,
    EventCapacity,
    EventOrder,
    EventOffset,
    InvalidGain,
    ClockOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Transport {
    position: u64,
    playing: bool,
}

impl Default for Transport {
    fn default() -> Self {
        Self::new()
    }
}

impl Transport {
    pub const fn new() -> Self {
        Self {
            position: 0,
            playing: false,
        }
    }

    pub const fn position(&self) -> u64 {
        self.position
    }

    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    pub fn play(&mut self) {
        self.playing = true;
    }
    pub fn stop(&mut self) {
        self.playing = false;
    }
    pub fn locate(&mut self, frame: u64) {
        self.position = frame;
    }
}

pub struct Engine {
    gain: f32,
    transport: Transport,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub const fn new() -> Self {
        Self {
            gain: 1.0,
            transport: Transport::new(),
        }
    }

    pub fn transport(&mut self) -> &mut Transport {
        &mut self.transport
    }

    /// Validate the entire block before changing state or writing output.
    /// Events take effect before the sample at their offset. Equal offsets
    /// retain input order. Offsets at the block end affect the next block.
    pub fn render(
        &mut self,
        input: &[[f32; 2]],
        output: &mut [[f32; 2]],
        events: &[GainEvent],
    ) -> Result<(), RenderError> {
        let frames = input.len();
        if frames != output.len() || frames > MAX_FRAMES {
            return Err(RenderError::BufferSize);
        }
        if events.len() > MAX_EVENTS {
            return Err(RenderError::EventCapacity);
        }
        let mut previous = 0;
        for event in events {
            if event.offset > frames {
                return Err(RenderError::EventOffset);
            }
            if event.offset < previous {
                return Err(RenderError::EventOrder);
            }
            if !event.gain.is_finite() || event.gain < 0.0 {
                return Err(RenderError::InvalidGain);
            }
            previous = event.offset;
        }
        let next_position = if self.transport.playing {
            self.transport
                .position
                .checked_add(frames as u64)
                .ok_or(RenderError::ClockOverflow)?
        } else {
            self.transport.position
        };
        let mut start = 0;
        for event in events {
            self.render_span(
                &input[start..event.offset],
                &mut output[start..event.offset],
            );
            self.gain = event.gain;
            start = event.offset;
        }
        self.render_span(&input[start..], &mut output[start..]);
        self.transport.position = next_position;
        Ok(())
    }

    fn render_span(&self, input: &[[f32; 2]], output: &mut [[f32; 2]]) {
        for (source, destination) in input.iter().zip(output.iter_mut()) {
            *destination = [source[0] * self.gain, source[1] * self.gain];
        }
    }
}
