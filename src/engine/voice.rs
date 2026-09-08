//! A polyphonic instrument.
//!
//! A [`VoiceBank`] holds a fixed number of voices and assigns notes to
//! them. Each voice is an oscillator through a filter with an amplitude
//! envelope, which is the smallest arrangement that sounds like an
//! instrument rather than a test tone.
//!
//! Everything is fixed-size and allocation-free, so a bank runs inside the
//! audio callback. When every voice is busy the quietest one is taken,
//! which is what a listener notices least.

use crate::dsp::biquad::{Biquad, Coefficients, Kind};
use crate::dsp::env::{Envelope, Settings, Stage};
use crate::dsp::osc::{Oscillator, Shape};
use crate::dsp::pan;

/// Voices one bank can sound at once.
pub const MAX_VOICES: usize = 32;
/// Lowest note number.
pub const MIN_PITCH: u8 = 0;
/// Highest note number.
pub const MAX_PITCH: u8 = 127;

/// How an instrument sounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    /// Waveform each voice produces.
    pub shape: Shape,
    /// Amplitude envelope.
    pub envelope: Settings,
    /// Filter cutoff in hertz.
    pub cutoff: f32,
    /// Filter resonance.
    pub resonance: f32,
    /// Level applied to every voice, in decibels.
    pub level_db: f32,
}

impl Default for Patch {
    fn default() -> Self {
        Self {
            shape: Shape::Saw,
            envelope: Settings {
                attack: 0.004,
                decay: 0.12,
                sustain: 0.65,
                release: 0.18,
            },
            cutoff: 4_000.0,
            resonance: 0.9,
            level_db: -12.0,
        }
    }
}

/// Frequency of a note number, with 69 at 440 hertz.
#[must_use]
pub fn pitch_to_hertz(pitch: u8) -> f32 {
    440.0 * (2.0_f32).powf((f32::from(pitch) - 69.0) / 12.0)
}

#[derive(Clone, Copy)]
struct Voice {
    oscillator: Oscillator,
    envelope: Envelope,
    filter: Biquad,
    pitch: u8,
    velocity: f32,
    /// Order the voice was started in, so the oldest can be identified.
    age: u64,
    active: bool,
}

impl Voice {
    fn new(sample_rate: f32, patch: &Patch) -> Self {
        Self {
            oscillator: Oscillator::new(patch.shape, 440.0, sample_rate),
            envelope: Envelope::new(patch.envelope, sample_rate),
            filter: Biquad::new(Coefficients::design(
                Kind::LowPass,
                patch.cutoff,
                patch.resonance,
                0.0,
                sample_rate,
            )),
            pitch: 0,
            velocity: 0.0,
            age: 0,
            active: false,
        }
    }

    #[inline]
    fn process(&mut self) -> f32 {
        if !self.active {
            return 0.0;
        }
        let level = self.envelope.process();
        if !self.envelope.is_active() {
            self.active = false;
            return 0.0;
        }
        let raw = self.oscillator.process();
        self.filter.process(raw * level * self.velocity)
    }
}

/// A fixed set of voices playing one patch.
pub struct VoiceBank {
    voices: [Voice; MAX_VOICES],
    patch: Patch,
    sample_rate: f32,
    gain: f32,
    counter: u64,
    /// Voices allowed to sound. Lower values cost less on a busy machine.
    polyphony: usize,
}

impl VoiceBank {
    /// A silent bank playing `patch`.
    #[must_use]
    pub fn new(patch: Patch, sample_rate: f32) -> Self {
        let sample_rate = if sample_rate > 0.0 {
            sample_rate
        } else {
            48_000.0
        };
        let mut bank = Self {
            voices: [Voice::new(sample_rate, &patch); MAX_VOICES],
            patch,
            sample_rate,
            gain: 1.0,
            counter: 0,
            polyphony: MAX_VOICES,
        };
        bank.set_patch(patch);
        bank
    }

    /// The patch in use.
    #[inline]
    #[must_use]
    pub const fn patch(&self) -> Patch {
        self.patch
    }

    /// Replaces the patch. Voices already sounding keep their envelope
    /// position and pick up the new filter and waveform.
    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
        self.gain = crate::dsp::db::to_linear(patch.level_db);
        let coefficients = Coefficients::design(
            Kind::LowPass,
            patch.cutoff,
            patch.resonance,
            0.0,
            self.sample_rate,
        );
        for voice in &mut self.voices {
            voice.oscillator.set_shape(patch.shape);
            voice.envelope.set_settings(patch.envelope);
            voice.filter.set_coefficients(coefficients);
        }
    }

    /// Voices allowed to sound at once.
    #[inline]
    #[must_use]
    pub const fn polyphony(&self) -> usize {
        self.polyphony
    }

    /// Limits how many voices may sound. Clamped to 1 through
    /// [`MAX_VOICES`]; voices past the new limit are silenced.
    pub fn set_polyphony(&mut self, voices: usize) {
        self.polyphony = voices.clamp(1, MAX_VOICES);
        for voice in &mut self.voices[self.polyphony..] {
            voice.active = false;
            voice.envelope.reset();
        }
    }

    /// Voices currently sounding.
    #[must_use]
    pub fn active_voices(&self) -> usize {
        self.voices[..self.polyphony]
            .iter()
            .filter(|voice| voice.active)
            .count()
    }

    /// Whether any voice is sounding.
    #[must_use]
    pub fn is_silent(&self) -> bool {
        self.active_voices() == 0
    }

    /// Starts a note. A note already sounding at that pitch is restarted
    /// rather than doubled, and when every voice is busy the quietest is
    /// taken.
    pub fn note_on(&mut self, pitch: u8, velocity: u8) {
        if velocity == 0 {
            self.note_off(pitch);
            return;
        }
        let pitch = pitch.clamp(MIN_PITCH, MAX_PITCH);
        self.counter += 1;
        let index = self.choose_voice(pitch);
        let counter = self.counter;
        let voice = &mut self.voices[index];
        voice.pitch = pitch;
        voice.velocity = f32::from(velocity) / 127.0;
        voice.age = counter;
        voice.active = true;
        voice.oscillator.set_frequency(pitch_to_hertz(pitch));
        // A voice taken from another note starts its waveform afresh so
        // the new note does not inherit a click from the old one.
        voice.oscillator.reset();
        voice.filter.reset();
        voice.envelope.note_on();
    }

    /// Releases every voice sounding at `pitch`.
    pub fn note_off(&mut self, pitch: u8) {
        for voice in &mut self.voices[..self.polyphony] {
            if voice.active && voice.pitch == pitch && voice.envelope.stage() != Stage::Release {
                voice.envelope.note_off();
            }
        }
    }

    /// Releases every note.
    pub fn all_notes_off(&mut self) {
        for voice in &mut self.voices[..self.polyphony] {
            if voice.active {
                voice.envelope.note_off();
            }
        }
    }

    /// Silences every voice at once, without a release.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.active = false;
            voice.envelope.reset();
            voice.oscillator.reset();
            voice.filter.reset();
        }
    }

    /// Picks the voice a new note should use: a voice already on that
    /// pitch, then a free one, then the quietest sounding one.
    fn choose_voice(&self, pitch: u8) -> usize {
        let voices = &self.voices[..self.polyphony];
        if let Some(index) = voices
            .iter()
            .position(|voice| voice.active && voice.pitch == pitch)
        {
            return index;
        }
        if let Some(index) = voices.iter().position(|voice| !voice.active) {
            return index;
        }
        // Every voice is busy. Take the quietest, and among equals the
        // oldest, which is the least noticeable to interrupt.
        let mut chosen = 0;
        let mut quietest = f32::INFINITY;
        let mut oldest = u64::MAX;
        for (index, voice) in voices.iter().enumerate() {
            let level = voice.envelope.value();
            if level < quietest || (level == quietest && voice.age < oldest) {
                chosen = index;
                quietest = level;
                oldest = voice.age;
            }
        }
        chosen
    }

    /// Adds this bank's output into `output`, panned by `position`.
    ///
    /// The signal is added rather than written, so several banks can share
    /// one buffer.
    pub fn render_additive(&mut self, output: &mut [[f32; 2]], position: f32) {
        let gains = pan::constant_power(position);
        let left = gains.left * core::f32::consts::SQRT_2 * self.gain;
        let right = gains.right * core::f32::consts::SQRT_2 * self.gain;
        for frame in output.iter_mut() {
            let mut sum = 0.0;
            for voice in &mut self.voices[..self.polyphony] {
                sum += voice.process();
            }
            frame[0] += sum * left;
            frame[1] += sum * right;
        }
    }

    /// Writes this bank's output into `output`, replacing it.
    pub fn render(&mut self, output: &mut [[f32; 2]], position: f32) {
        output.fill([0.0, 0.0]);
        self.render_additive(output, position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn peak(samples: &[[f32; 2]]) -> f32 {
        samples.iter().fold(0.0_f32, |worst, frame| {
            worst.max(frame[0].abs()).max(frame[1].abs())
        })
    }

    #[test]
    fn note_numbers_map_to_the_expected_frequencies() {
        assert!((pitch_to_hertz(69) - 440.0).abs() < 1e-3);
        assert!((pitch_to_hertz(57) - 220.0).abs() < 1e-3);
        assert!((pitch_to_hertz(81) - 880.0).abs() < 1e-2);
        assert!((pitch_to_hertz(60) - 261.625_5).abs() < 0.01);
        // The whole range stays audible and finite.
        for pitch in MIN_PITCH..=MAX_PITCH {
            let hertz = pitch_to_hertz(pitch);
            assert!(hertz.is_finite() && hertz > 0.0, "{pitch}");
        }
    }

    #[test]
    fn a_new_bank_is_silent() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        assert!(bank.is_silent());
        assert_eq!(bank.active_voices(), 0);
        let mut output = [[0.0_f32; 2]; 256];
        bank.render(&mut output, 0.0);
        assert_eq!(peak(&output), 0.0);
    }

    #[test]
    fn a_note_sounds_and_stops_after_its_release() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 100);
        assert_eq!(bank.active_voices(), 1);
        let mut output = [[0.0_f32; 2]; 4_800];
        bank.render(&mut output, 0.0);
        assert!(peak(&output) > 0.001, "{}", peak(&output));

        bank.note_off(60);
        // The release is 180 ms; a second of rendering finishes it.
        let mut tail = [[0.0_f32; 2]; 48_000];
        bank.render(&mut tail, 0.0);
        assert!(bank.is_silent(), "{} voices left", bank.active_voices());
        let mut after = [[0.0_f32; 2]; 256];
        bank.render(&mut after, 0.0);
        assert_eq!(peak(&after), 0.0);
    }

    #[test]
    fn a_note_with_no_velocity_releases_instead_of_starting() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(64, 100);
        let mut output = [[0.0_f32; 2]; 512];
        bank.render(&mut output, 0.0);
        bank.note_on(64, 0);
        let mut tail = [[0.0_f32; 2]; 48_000];
        bank.render(&mut tail, 0.0);
        assert!(bank.is_silent());
    }

    #[test]
    fn several_notes_sound_together() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        for pitch in [60, 64, 67, 72] {
            bank.note_on(pitch, 100);
        }
        assert_eq!(bank.active_voices(), 4);
        let mut output = [[0.0_f32; 2]; 2_048];
        bank.render(&mut output, 0.0);
        assert!(peak(&output) > 0.001);
        bank.note_off(64);
        // Releasing one leaves the others sounding.
        let mut brief = [[0.0_f32; 2]; 256];
        bank.render(&mut brief, 0.0);
        assert!(bank.active_voices() >= 3);
    }

    #[test]
    fn the_same_pitch_twice_restarts_one_voice_rather_than_doubling() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 100);
        bank.note_on(60, 100);
        bank.note_on(60, 100);
        assert_eq!(bank.active_voices(), 1);
    }

    #[test]
    fn a_busy_bank_takes_the_quietest_voice() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.set_polyphony(4);
        for pitch in [60, 62, 64, 65] {
            bank.note_on(pitch, 100);
        }
        assert_eq!(bank.active_voices(), 4);
        // Let one note fall well into its release so it is the quietest.
        bank.note_off(60);
        let mut settle = [[0.0_f32; 2]; 4_000];
        bank.render(&mut settle, 0.0);

        bank.note_on(72, 100);
        // Still four voices: the releasing one was taken, not a fifth added.
        assert_eq!(bank.active_voices(), 4);
        let mut output = [[0.0_f32; 2]; 1_024];
        bank.render(&mut output, 0.0);
        assert!(peak(&output) > 0.0);
    }

    #[test]
    fn polyphony_limits_how_many_voices_sound() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.set_polyphony(2);
        assert_eq!(bank.polyphony(), 2);
        for pitch in [60, 62, 64, 65, 67] {
            bank.note_on(pitch, 100);
        }
        assert_eq!(bank.active_voices(), 2);
        // The limit is clamped to something usable.
        bank.set_polyphony(0);
        assert_eq!(bank.polyphony(), 1);
        bank.set_polyphony(1_000);
        assert_eq!(bank.polyphony(), MAX_VOICES);
    }

    #[test]
    fn lowering_polyphony_silences_the_voices_beyond_it() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        for pitch in 60..70 {
            bank.note_on(pitch, 100);
        }
        assert_eq!(bank.active_voices(), 10);
        bank.set_polyphony(3);
        assert_eq!(bank.active_voices(), 3);
    }

    #[test]
    fn all_notes_off_releases_everything() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        for pitch in 60..68 {
            bank.note_on(pitch, 100);
        }
        bank.all_notes_off();
        let mut tail = [[0.0_f32; 2]; 48_000];
        bank.render(&mut tail, 0.0);
        assert!(bank.is_silent());
    }

    #[test]
    fn reset_silences_without_a_release() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 127);
        bank.reset();
        assert!(bank.is_silent());
        let mut output = [[0.0_f32; 2]; 256];
        bank.render(&mut output, 0.0);
        assert_eq!(peak(&output), 0.0);
    }

    #[test]
    fn velocity_changes_the_level() {
        let render_at = |velocity: u8| {
            let mut bank = VoiceBank::new(Patch::default(), RATE);
            bank.note_on(60, velocity);
            let mut output = [[0.0_f32; 2]; 4_800];
            bank.render(&mut output, 0.0);
            peak(&output)
        };
        let quiet = render_at(30);
        let loud = render_at(127);
        assert!(loud > quiet * 2.0, "quiet {quiet}, loud {loud}");
    }

    #[test]
    fn panning_moves_the_output_between_the_channels() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 100);
        let mut output = [[0.0_f32; 2]; 4_800];
        bank.render(&mut output, -1.0);
        let left = output
            .iter()
            .fold(0.0_f32, |worst, frame| worst.max(frame[0].abs()));
        let right = output
            .iter()
            .fold(0.0_f32, |worst, frame| worst.max(frame[1].abs()));
        assert!(left > 0.001);
        assert!(right < 1e-6, "{right}");
    }

    #[test]
    fn rendering_adds_rather_than_replaces() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 100);
        let mut output = [[0.5_f32, 0.5]; 512];
        bank.render_additive(&mut output, 0.0);
        // The existing content survived.
        assert!(output.iter().all(|frame| frame[0] != 0.0));
        let mut replaced = [[0.5_f32, 0.5]; 512];
        bank.render(&mut replaced, 0.0);
        assert!(replaced[0][0].abs() < 0.5);
    }

    #[test]
    fn the_output_stays_bounded_with_every_voice_sounding() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        for pitch in 40..40 + MAX_VOICES as u8 {
            bank.note_on(pitch, 127);
        }
        assert_eq!(bank.active_voices(), MAX_VOICES);
        let mut output = [[0.0_f32; 2]; 4_800];
        bank.render(&mut output, 0.0);
        assert!(
            output
                .iter()
                .all(|frame| frame[0].is_finite() && frame[1].is_finite())
        );
        // Thirty-two saws at full velocity must not run away.
        assert!(peak(&output) < 8.0, "{}", peak(&output));
    }

    #[test]
    fn a_patch_change_reaches_sounding_voices() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(60, 100);
        // Let the envelope reach sustain so the comparison is about the
        // filter rather than the attack.
        let mut settle = [[0.0_f32; 2]; 14_400];
        bank.render(&mut settle, 0.0);
        let mut bright = [[0.0_f32; 2]; 4_800];
        bank.render(&mut bright, 0.0);

        let dull = Patch {
            cutoff: 200.0,
            ..Patch::default()
        };
        bank.set_patch(dull);
        assert_eq!(bank.patch().cutoff, 200.0);
        // Let the new filter settle before measuring.
        bank.render(&mut settle, 0.0);
        let mut filtered = [[0.0_f32; 2]; 4_800];
        bank.render(&mut filtered, 0.0);
        // A much lower cutoff takes energy out of a saw.
        assert!(
            peak(&filtered) < peak(&bright),
            "filtered {} was not quieter than bright {}",
            peak(&filtered),
            peak(&bright)
        );
    }

    #[test]
    fn every_waveform_sounds() {
        for shape in [Shape::Sine, Shape::Saw, Shape::Square, Shape::Triangle] {
            let patch = Patch {
                shape,
                ..Patch::default()
            };
            let mut bank = VoiceBank::new(patch, RATE);
            bank.note_on(60, 110);
            let mut output = [[0.0_f32; 2]; 9_600];
            bank.render(&mut output, 0.0);
            assert!(peak(&output) > 0.0005, "{shape:?}: {}", peak(&output));
        }
    }

    #[test]
    fn out_of_range_pitches_are_handled() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_on(MAX_PITCH, 100);
        bank.note_on(MIN_PITCH, 100);
        assert_eq!(bank.active_voices(), 2);
        let mut output = [[0.0_f32; 2]; 1_024];
        bank.render(&mut output, 0.0);
        assert!(output.iter().all(|frame| frame[0].is_finite()));
    }

    #[test]
    fn releasing_a_note_that_is_not_sounding_does_nothing() {
        let mut bank = VoiceBank::new(Patch::default(), RATE);
        bank.note_off(60);
        assert!(bank.is_silent());
        bank.note_on(60, 100);
        bank.note_off(72);
        assert_eq!(bank.active_voices(), 1);
    }
}
