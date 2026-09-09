//! Musical time.
//!
//! The transport converts between frames, the unit the audio device
//! counts in, and beats, the unit music is written in. It advances by
//! whole blocks so a render is reproducible: the same starting position
//! and the same block length always produce the same span of musical
//! time, whatever the machine is doing.
//!
//! Everything here is arithmetic on plain numbers. No method allocates or
//! blocks, so the transport lives on the audio thread with the mixer.

/// Smallest tempo accepted, in beats per minute.
pub const MIN_TEMPO: f64 = 20.0;
/// Largest tempo accepted, in beats per minute.
pub const MAX_TEMPO: f64 = 999.0;
/// Largest number of arrangement tempo changes accepted by one project.
pub const MAX_TEMPO_CHANGES: usize = 1_024;

/// A tempo change at an arrangement beat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TempoChange {
    /// Quarter-note beat where the new tempo starts.
    pub beat: f64,
    /// Tempo in beats per minute from this point forward.
    pub tempo: f64,
}

/// Borrowed piecewise-constant tempo map.
#[derive(Clone, Copy, Debug)]
pub struct TempoMap<'a> {
    initial: f64,
    changes: &'a [TempoChange],
}

impl<'a> TempoMap<'a> {
    /// Builds a view over sorted, validated changes after beat zero.
    #[must_use]
    pub const fn new(initial: f64, changes: &'a [TempoChange]) -> Self {
        Self { initial, changes }
    }

    /// Tempo active at `beat`.
    #[must_use]
    pub fn tempo_at(&self, beat: f64) -> f64 {
        let index = self.changes.partition_point(|change| change.beat <= beat);
        index
            .checked_sub(1)
            .map_or(self.initial, |index| self.changes[index].tempo)
    }

    /// First change strictly after `beat`.
    #[must_use]
    pub fn next_after(&self, beat: f64) -> Option<TempoChange> {
        self.changes
            .get(self.changes.partition_point(|change| change.beat <= beat))
            .copied()
    }

    /// Number of frames between two beat positions.
    #[must_use]
    pub fn frames_between(&self, start: f64, end: f64, sample_rate: f64) -> f64 {
        if !start.is_finite()
            || !end.is_finite()
            || end <= start
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return 0.0;
        }
        let mut beat = start;
        let mut frames = 0.0;
        while beat < end {
            let tempo = self.tempo_at(beat);
            let boundary = self
                .next_after(beat)
                .map_or(end, |change| change.beat.min(end));
            if boundary <= beat {
                break;
            }
            frames += (boundary - beat) * sample_rate * 60.0 / tempo;
            beat = boundary;
        }
        frames
    }

    /// Timeline frame corresponding to a beat position.
    #[must_use]
    pub fn frame_at(&self, beat: f64, sample_rate: f64) -> f64 {
        self.frames_between(0.0, beat.max(0.0), sample_rate)
    }

    /// Beat reached after advancing a number of frames.
    #[must_use]
    pub fn beat_after_frames(&self, start: f64, frames: f64, sample_rate: f64) -> f64 {
        if !start.is_finite()
            || !frames.is_finite()
            || frames <= 0.0
            || !sample_rate.is_finite()
            || sample_rate <= 0.0
        {
            return start.max(0.0);
        }
        let mut beat = start.max(0.0);
        let mut remaining = frames;
        loop {
            let tempo = self.tempo_at(beat);
            let frames_per_beat = sample_rate * 60.0 / tempo;
            let Some(change) = self.next_after(beat) else {
                return beat + remaining / frames_per_beat;
            };
            let to_change = (change.beat - beat) * frames_per_beat;
            if remaining < to_change {
                return beat + remaining / frames_per_beat;
            }
            remaining -= to_change;
            beat = change.beat;
            if remaining <= 0.0 {
                return beat;
            }
        }
    }
}

/// A musical position expressed the way a display shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarsBeats {
    /// Bar number, counting from one.
    pub bar: u32,
    /// Beat within the bar, counting from one.
    pub beat: u32,
    /// Sixteenth within the beat, counting from one.
    pub sixteenth: u32,
}

/// A loop over a span of musical time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoopRange {
    /// First beat played.
    pub start_beats: f64,
    /// Beats the loop spans. Zero or less disables the loop.
    pub length_beats: f64,
}

impl LoopRange {
    /// Whether the range would actually repeat.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.length_beats > 0.0
            && self.start_beats.is_finite()
            && self.length_beats.is_finite()
            && self.start_beats >= 0.0
    }

    /// First beat after the loop.
    #[must_use]
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }
}

/// Why a time signature was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeSignatureError {
    /// The beats per bar are outside 1 to 64.
    Numerator,
    /// The note value is not a power of two from 1 to 64.
    Denominator,
}

impl core::fmt::Display for TimeSignatureError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Numerator => formatter.write_str("beats per bar must be 1 to 64"),
            Self::Denominator => {
                formatter.write_str("the note value must be a power of two from 1 to 64")
            }
        }
    }
}

impl core::error::Error for TimeSignatureError {}

/// Position, tempo, and time signature.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transport {
    sample_rate: f64,
    tempo: f64,
    numerator: u16,
    denominator: u16,
    position_frames: u64,
    position_beats: f64,
    playing: bool,
    loop_range: LoopRange,
}

impl Transport {
    /// A stopped transport at the start of the timeline.
    ///
    /// A sample rate that is not positive falls back to 48 kHz, and a
    /// tempo outside the accepted range is clamped into it.
    #[must_use]
    pub fn new(sample_rate: f64, tempo: f64) -> Self {
        let mut transport = Self {
            sample_rate: if sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            tempo: 120.0,
            numerator: 4,
            denominator: 4,
            position_frames: 0,
            position_beats: 0.0,
            playing: false,
            loop_range: LoopRange {
                start_beats: 0.0,
                length_beats: 0.0,
            },
        };
        transport.set_tempo(tempo);
        transport
    }

    /// Frames per second.
    #[inline]
    #[must_use]
    pub const fn sample_rate(&self) -> f64 {
        self.sample_rate
    }

    /// Changes the sample rate, keeping the musical position. The frame
    /// position is recomputed from the beat, so a device change does not
    /// move the playhead in musical terms.
    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.position_frames = (self.position_beats * self.frames_per_beat()) as u64;
        }
    }

    /// Tempo in beats per minute.
    #[inline]
    #[must_use]
    pub const fn tempo(&self) -> f64 {
        self.tempo
    }

    /// Sets the tempo, clamped into the accepted range. A change takes
    /// effect from the current position; time already elapsed keeps the
    /// beats it was played at.
    pub fn set_tempo(&mut self, tempo: f64) {
        if tempo.is_nan() {
            return;
        }
        self.tempo = tempo.clamp(MIN_TEMPO, MAX_TEMPO);
    }

    /// Beats in a bar and the note value that gets the beat.
    #[inline]
    #[must_use]
    pub const fn time_signature(&self) -> (u16, u16) {
        (self.numerator, self.denominator)
    }

    /// Sets the time signature. A numerator outside 1 to 64, or a
    /// denominator that is not a power of two from 1 to 64, is refused and
    /// the signature is left alone.
    ///
    /// # Errors
    ///
    /// Returns [`TimeSignatureError`] when the signature is not usable.
    pub fn set_time_signature(
        &mut self,
        numerator: u16,
        denominator: u16,
    ) -> Result<(), TimeSignatureError> {
        if !(1..=64).contains(&numerator) {
            return Err(TimeSignatureError::Numerator);
        }
        if !matches!(denominator, 1 | 2 | 4 | 8 | 16 | 32 | 64) {
            return Err(TimeSignatureError::Denominator);
        }
        self.numerator = numerator;
        self.denominator = denominator;
        Ok(())
    }

    /// Frames in one beat at the current tempo.
    #[inline]
    #[must_use]
    pub fn frames_per_beat(&self) -> f64 {
        self.sample_rate * 60.0 / self.tempo
    }

    /// Beats in one bar of the current signature, in quarter notes.
    #[inline]
    #[must_use]
    pub fn beats_per_bar(&self) -> f64 {
        f64::from(self.numerator) * 4.0 / f64::from(self.denominator)
    }

    /// Whether the transport is running.
    #[inline]
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.playing
    }

    /// Starts playing from the current position.
    #[inline]
    pub fn play(&mut self) {
        self.playing = true;
    }

    /// Stops, keeping the position.
    #[inline]
    pub fn stop(&mut self) {
        self.playing = false;
    }

    /// Position in frames since the start of the timeline.
    #[inline]
    #[must_use]
    pub const fn position_frames(&self) -> u64 {
        self.position_frames
    }

    /// Position in beats since the start of the timeline.
    #[inline]
    #[must_use]
    pub const fn position_beats(&self) -> f64 {
        self.position_beats
    }

    /// Moves the playhead to a beat, whether or not it is playing.
    pub fn locate_beats(&mut self, beats: f64) {
        if beats.is_nan() {
            return;
        }
        let beats = beats.max(0.0);
        self.position_beats = beats;
        self.position_frames = (beats * self.frames_per_beat()) as u64;
    }

    /// Moves the playhead to a frame.
    pub fn locate_frames(&mut self, frames: u64) {
        self.position_frames = frames;
        self.position_beats = frames as f64 / self.frames_per_beat();
    }

    /// Moves the playhead using positions calculated by an external tempo map.
    pub fn locate_mapped(&mut self, beats: f64, frames: u64) {
        if beats.is_finite() && beats >= 0.0 {
            self.position_beats = beats;
            self.position_frames = frames;
        }
    }

    /// Loop currently set.
    #[inline]
    #[must_use]
    pub const fn loop_range(&self) -> LoopRange {
        self.loop_range
    }

    /// Sets the loop. A length of zero or less turns looping off.
    pub fn set_loop(&mut self, range: LoopRange) {
        self.loop_range = range;
    }

    /// The span `frames` would cover, without moving the playhead.
    ///
    /// Returns `(first beat, beats elapsed)`, matching what
    /// [`advance`](Self::advance) reports, so a caller can schedule a
    /// block before playing it.
    #[must_use]
    pub fn peek(&self, frames: usize) -> (f64, f64) {
        let start = self.position_beats;
        if !self.playing || frames == 0 {
            return (start, 0.0);
        }
        let elapsed = frames as f64 / self.frames_per_beat();
        if self.loop_range.is_active() {
            let loop_end = self.loop_range.end_beats();
            if start < loop_end && start + elapsed >= loop_end {
                return (start, loop_end - start);
            }
        }
        (start, elapsed)
    }

    /// Advances by `frames`, returning the span of musical time the block
    /// covers as `(first beat, beats elapsed)`.
    ///
    /// A stopped transport reports its position and no elapsed time. When
    /// a loop is active and the block would run past its end, the playhead
    /// wraps to the loop start; the reported span is the part played
    /// before the wrap, and the position afterwards is inside the loop.
    pub fn advance(&mut self, frames: usize) -> (f64, f64) {
        let start = self.position_beats;
        if !self.playing || frames == 0 {
            return (start, 0.0);
        }
        let elapsed = frames as f64 / self.frames_per_beat();
        let end = start + elapsed;
        if self.loop_range.is_active() {
            let loop_end = self.loop_range.end_beats();
            if start < loop_end && end >= loop_end {
                // Wrap, carrying the overshoot past the loop point.
                let played = loop_end - start;
                let overshoot = end - loop_end;
                let length = self.loop_range.length_beats;
                let wrapped = self.loop_range.start_beats + overshoot % length;
                self.locate_beats(wrapped);
                return (start, played);
            }
        }
        self.position_frames = self.position_frames.saturating_add(frames as u64);
        self.position_beats = end;
        (start, elapsed)
    }

    /// The current position as bars, beats, and sixteenths.
    #[must_use]
    pub fn bars_beats(&self) -> BarsBeats {
        self.bars_beats_at(self.position_beats)
    }

    /// A beat position as bars, beats, and sixteenths.
    #[must_use]
    pub fn bars_beats_at(&self, beats: f64) -> BarsBeats {
        let beats = if beats.is_finite() {
            beats.max(0.0)
        } else {
            0.0
        };
        let per_bar = self.beats_per_bar();
        let bar = (beats / per_bar).floor();
        let within_bar = beats - bar * per_bar;
        let beat = within_bar.floor();
        let sixteenth = ((within_bar - beat) * 4.0).floor();
        BarsBeats {
            bar: bar as u32 + 1,
            beat: beat as u32 + 1,
            sixteenth: sixteenth as u32 + 1,
        }
    }

    /// Frames from the start of the timeline to `beats`.
    #[must_use]
    pub fn beats_to_frames(&self, beats: f64) -> f64 {
        beats * self.frames_per_beat()
    }

    /// Beats from the start of the timeline to `frames`.
    #[must_use]
    pub fn frames_to_beats(&self, frames: f64) -> f64 {
        frames / self.frames_per_beat()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn a_new_transport_is_stopped_at_the_start() {
        let transport = Transport::new(RATE, 120.0);
        assert!(!transport.is_playing());
        assert_eq!(transport.position_frames(), 0);
        assert_eq!(transport.position_beats(), 0.0);
        assert_eq!(transport.tempo(), 120.0);
        assert_eq!(transport.time_signature(), (4, 4));
        assert_eq!(transport.sample_rate(), RATE);
    }

    #[test]
    fn tempo_is_clamped_and_nan_is_ignored() {
        let mut transport = Transport::new(RATE, 5.0);
        assert_eq!(transport.tempo(), MIN_TEMPO);
        transport.set_tempo(10_000.0);
        assert_eq!(transport.tempo(), MAX_TEMPO);
        transport.set_tempo(128.0);
        transport.set_tempo(f64::NAN);
        assert_eq!(transport.tempo(), 128.0);
    }

    #[test]
    fn a_sample_rate_that_is_not_positive_is_refused() {
        let transport = Transport::new(0.0, 120.0);
        assert_eq!(transport.sample_rate(), 48_000.0);
        let mut transport = Transport::new(RATE, 120.0);
        transport.set_sample_rate(-1.0);
        assert_eq!(transport.sample_rate(), RATE);
    }

    #[test]
    fn frames_and_beats_convert_both_ways() {
        let transport = Transport::new(RATE, 120.0);
        // At 120 bpm a beat is half a second, so 24000 frames.
        assert!(close(transport.frames_per_beat(), 24_000.0));
        assert!(close(transport.beats_to_frames(4.0), 96_000.0));
        assert!(close(transport.frames_to_beats(96_000.0), 4.0));
        for beats in [0.0, 0.25, 1.0, 7.5, 1_000.0] {
            let frames = transport.beats_to_frames(beats);
            assert!(close(transport.frames_to_beats(frames), beats), "{beats}");
        }
    }

    #[test]
    fn a_stopped_transport_does_not_advance() {
        let mut transport = Transport::new(RATE, 120.0);
        assert_eq!(transport.advance(512), (0.0, 0.0));
        assert_eq!(transport.position_frames(), 0);
        transport.locate_beats(4.0);
        assert_eq!(transport.advance(512), (4.0, 0.0));
    }

    #[test]
    fn playing_advances_by_whole_blocks() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.play();
        // Half a beat per 12000 frames.
        let (start, elapsed) = transport.advance(12_000);
        assert!(close(start, 0.0));
        assert!(close(elapsed, 0.5));
        assert_eq!(transport.position_frames(), 12_000);
        assert!(close(transport.position_beats(), 0.5));

        let (start, elapsed) = transport.advance(12_000);
        assert!(close(start, 0.5));
        assert!(close(elapsed, 0.5));
        assert!(close(transport.position_beats(), 1.0));
    }

    #[test]
    fn advancing_no_frames_reports_no_time() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.play();
        assert_eq!(transport.advance(0), (0.0, 0.0));
        assert_eq!(transport.position_frames(), 0);
    }

    #[test]
    fn the_same_blocks_always_give_the_same_positions() {
        let run = || {
            let mut transport = Transport::new(RATE, 137.5);
            transport.play();
            let mut spans = Vec::new();
            for _ in 0..100 {
                spans.push(transport.advance(256));
            }
            (
                spans,
                transport.position_beats(),
                transport.position_frames(),
            )
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn locating_moves_frames_and_beats_together() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.locate_beats(3.0);
        assert!(close(transport.position_beats(), 3.0));
        assert_eq!(transport.position_frames(), 72_000);

        transport.locate_frames(24_000);
        assert_eq!(transport.position_frames(), 24_000);
        assert!(close(transport.position_beats(), 1.0));

        // A position before the start is clamped, and NaN is ignored.
        transport.locate_beats(-5.0);
        assert_eq!(transport.position_beats(), 0.0);
        transport.locate_beats(2.0);
        transport.locate_beats(f64::NAN);
        assert!(close(transport.position_beats(), 2.0));
    }

    #[test]
    fn changing_the_sample_rate_keeps_the_musical_position() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.locate_beats(8.0);
        transport.set_sample_rate(96_000.0);
        assert!(close(transport.position_beats(), 8.0));
        assert_eq!(transport.position_frames(), 8 * 48_000);
    }

    #[test]
    fn a_tempo_change_alters_the_rate_of_travel() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.play();
        transport.advance(24_000);
        assert!(close(transport.position_beats(), 1.0));
        // Twice the tempo covers twice the beats in the same frames.
        transport.set_tempo(240.0);
        let (_, elapsed) = transport.advance(24_000);
        assert!(close(elapsed, 2.0));
        assert!(close(transport.position_beats(), 3.0));
    }

    #[test]
    fn a_loop_wraps_at_its_end() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.set_loop(LoopRange {
            start_beats: 4.0,
            length_beats: 4.0,
        });
        transport.locate_beats(7.5);
        transport.play();
        // Half a beat to the loop end, then a quarter beat past it.
        let (start, played) = transport.advance(18_000);
        assert!(close(start, 7.5));
        assert!(close(played, 0.5));
        assert!(close(transport.position_beats(), 4.25));
    }

    #[test]
    fn a_loop_shorter_than_a_block_still_lands_inside_itself() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.set_loop(LoopRange {
            start_beats: 0.0,
            length_beats: 0.25,
        });
        transport.play();
        for _ in 0..20 {
            transport.advance(12_000);
            let beats = transport.position_beats();
            assert!(
                (0.0..0.25).contains(&beats),
                "position {beats} left the loop"
            );
        }
    }

    #[test]
    fn an_inactive_loop_is_ignored() {
        let mut transport = Transport::new(RATE, 120.0);
        for range in [
            LoopRange {
                start_beats: 0.0,
                length_beats: 0.0,
            },
            LoopRange {
                start_beats: 0.0,
                length_beats: -4.0,
            },
            LoopRange {
                start_beats: -1.0,
                length_beats: 4.0,
            },
            LoopRange {
                start_beats: f64::NAN,
                length_beats: 4.0,
            },
        ] {
            assert!(!range.is_active(), "{range:?}");
            let mut transport = transport;
            transport.set_loop(range);
            transport.play();
            transport.advance(48_000);
            assert!(transport.position_beats() > 1.0);
        }
        transport.play();
    }

    #[test]
    fn a_position_before_the_loop_plays_into_it() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.set_loop(LoopRange {
            start_beats: 8.0,
            length_beats: 4.0,
        });
        transport.play();
        // Starting at zero, the transport plays forward without wrapping
        // until it reaches the loop end.
        let (_, elapsed) = transport.advance(24_000);
        assert!(close(elapsed, 1.0));
        assert!(close(transport.position_beats(), 1.0));
    }

    #[test]
    fn time_signatures_are_checked() {
        let mut transport = Transport::new(RATE, 120.0);
        assert_eq!(transport.set_time_signature(3, 4), Ok(()));
        assert_eq!(transport.time_signature(), (3, 4));
        assert!(close(transport.beats_per_bar(), 3.0));
        assert_eq!(transport.set_time_signature(7, 8), Ok(()));
        assert!(close(transport.beats_per_bar(), 3.5));
        assert_eq!(
            transport.set_time_signature(0, 4),
            Err(TimeSignatureError::Numerator)
        );
        assert_eq!(
            transport.set_time_signature(4, 5),
            Err(TimeSignatureError::Denominator)
        );
        assert_eq!(
            transport.set_time_signature(4, 128),
            Err(TimeSignatureError::Denominator)
        );
        assert_eq!(
            transport.set_time_signature(65, 4),
            Err(TimeSignatureError::Numerator)
        );
        assert!(!TimeSignatureError::Numerator.to_string().is_empty());
        assert!(!TimeSignatureError::Denominator.to_string().is_empty());
        // The refused changes left the signature alone.
        assert_eq!(transport.time_signature(), (7, 8));
    }

    #[test]
    fn bars_and_beats_count_from_one() {
        let mut transport = Transport::new(RATE, 120.0);
        assert_eq!(
            transport.bars_beats(),
            BarsBeats {
                bar: 1,
                beat: 1,
                sixteenth: 1
            }
        );
        transport.locate_beats(4.0);
        assert_eq!(
            transport.bars_beats(),
            BarsBeats {
                bar: 2,
                beat: 1,
                sixteenth: 1
            }
        );
        transport.locate_beats(6.5);
        assert_eq!(
            transport.bars_beats(),
            BarsBeats {
                bar: 2,
                beat: 3,
                sixteenth: 3
            }
        );
    }

    #[test]
    fn bars_and_beats_follow_the_signature() {
        let mut transport = Transport::new(RATE, 120.0);
        transport.set_time_signature(3, 4).unwrap();
        transport.locate_beats(3.0);
        assert_eq!(transport.bars_beats().bar, 2);
        transport.set_time_signature(6, 8).unwrap();
        // Six eighths is three quarter notes to the bar.
        transport.locate_beats(3.0);
        assert_eq!(transport.bars_beats().bar, 2);
    }

    #[test]
    fn odd_positions_report_a_usable_bar() {
        let transport = Transport::new(RATE, 120.0);
        for beats in [f64::NAN, f64::INFINITY, -12.0] {
            let position = transport.bars_beats_at(beats);
            assert!(position.bar >= 1, "{beats}: {position:?}");
            assert!((1..=4).contains(&position.beat), "{beats}: {position:?}");
        }
    }

    #[test]
    fn tempo_maps_convert_both_directions_across_changes() {
        let changes = [
            TempoChange {
                beat: 4.0,
                tempo: 60.0,
            },
            TempoChange {
                beat: 8.0,
                tempo: 240.0,
            },
        ];
        let map = TempoMap::new(120.0, &changes);
        assert_eq!(map.tempo_at(0.0), 120.0);
        assert_eq!(map.tempo_at(4.0), 60.0);
        assert_eq!(map.tempo_at(9.0), 240.0);
        assert_eq!(map.next_after(4.0), Some(changes[1]));
        let frame = map.frame_at(10.0, RATE);
        assert!(close(frame, 312_000.0));
        assert!(close(map.beat_after_frames(0.0, frame, RATE), 10.0));
        assert!(close(map.beat_after_frames(3.0, 72_000.0, RATE), 5.0));
    }

    #[test]
    fn tempo_map_rejects_unusable_conversion_inputs() {
        let map = TempoMap::new(120.0, &[]);
        assert_eq!(map.frames_between(2.0, 1.0, RATE), 0.0);
        assert_eq!(map.frames_between(0.0, 1.0, 0.0), 0.0);
        assert_eq!(map.beat_after_frames(3.0, -1.0, RATE), 3.0);
    }
}
