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
/// Detuned oscillator copies one voice can render.
pub const MAX_UNISON: usize = 4;
/// Lowest note number.
pub const MIN_PITCH: u8 = 0;
/// Highest note number.
pub const MAX_PITCH: u8 = 127;

/// How an instrument sounds.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Patch {
    /// Primary oscillator waveform.
    pub shape: Shape,
    /// Secondary oscillator waveform.
    pub shape_b: Shape,
    /// Secondary oscillator share, from zero to one.
    pub oscillator_mix: f32,
    /// Secondary oscillator tuning relative to the primary oscillator.
    pub oscillator_b_detune_cents: f32,
    /// Sub oscillator level, from zero to one.
    pub sub_level: f32,
    /// White noise level, from zero to one.
    pub noise_level: f32,
    /// Detuned copies per oscillator.
    pub unison_voices: u8,
    /// Spacing between unison copies in cents.
    pub unison_detune_cents: f32,
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
            shape_b: Shape::Square,
            oscillator_mix: 0.0,
            oscillator_b_detune_cents: 7.0,
            sub_level: 0.0,
            noise_level: 0.0,
            unison_voices: 1,
            unison_detune_cents: 12.0,
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

impl Patch {
    /// Whether every parameter is inside the persisted patch range.
    #[must_use]
    pub fn is_valid(self) -> bool {
        let envelope = self.envelope;
        (0.0..=1.0).contains(&self.oscillator_mix)
            && self.oscillator_b_detune_cents.is_finite()
            && (-2_400.0..=2_400.0).contains(&self.oscillator_b_detune_cents)
            && (0.0..=1.0).contains(&self.sub_level)
            && (0.0..=1.0).contains(&self.noise_level)
            && (1..=MAX_UNISON as u8).contains(&self.unison_voices)
            && self.unison_detune_cents.is_finite()
            && (0.0..=100.0).contains(&self.unison_detune_cents)
            && [envelope.attack, envelope.decay, envelope.release]
                .iter()
                .all(|value| value.is_finite() && (0.0..=60.0).contains(value))
            && envelope.sustain.is_finite()
            && (0.0..=1.0).contains(&envelope.sustain)
            && self.cutoff.is_finite()
            && (20.0..=20_000.0).contains(&self.cutoff)
            && self.resonance.is_finite()
            && (0.001..=100.0).contains(&self.resonance)
            && (self.level_db == f32::NEG_INFINITY
                || (self.level_db.is_finite() && (-120.0..=6.0).contains(&self.level_db)))
    }
}

/// Frequency of a note number, with 69 at 440 hertz.
#[must_use]
pub fn pitch_to_hertz(pitch: u8) -> f32 {
    440.0 * (2.0_f32).powf((f32::from(pitch) - 69.0) / 12.0)
}

#[derive(Clone, Copy)]
struct Voice {
    oscillator_a: [Oscillator; MAX_UNISON],
    oscillator_b: [Oscillator; MAX_UNISON],
    sub: Oscillator,
    envelope: Envelope,
    filter: Biquad,
    pitch: u8,
    velocity: f32,
    /// Order the voice was started in, so the oldest can be identified.
    age: u64,
    active: bool,
    noise_state: u64,
}

impl Voice {
    fn new(sample_rate: f32, patch: &Patch) -> Self {
        Self {
            oscillator_a: [Oscillator::new(patch.shape, 440.0, sample_rate); MAX_UNISON],
            oscillator_b: [Oscillator::new(patch.shape_b, 440.0, sample_rate); MAX_UNISON],
            sub: Oscillator::new(Shape::Sine, 220.0, sample_rate),
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
            noise_state: 1,
        }
    }

    #[inline]
    fn process(&mut self, patch: &Patch) -> f32 {
        if !self.active {
            return 0.0;
        }
        let level = self.envelope.process();
        if !self.envelope.is_active() {
            self.active = false;
            return 0.0;
        }
        let count = usize::from(patch.unison_voices);
        let mut oscillator_a = 0.0;
        let render_b = patch.oscillator_mix > 0.0;
        let mut oscillator_b = 0.0;
        for oscillator in &mut self.oscillator_a[..count] {
            oscillator_a += oscillator.process();
        }
        if render_b {
            for oscillator in &mut self.oscillator_b[..count] {
                oscillator_b += oscillator.process();
            }
        }
        let count = f32::from(patch.unison_voices);
        oscillator_a /= count;
        if render_b {
            oscillator_b /= count;
        }
        let pitched = oscillator_a + (oscillator_b - oscillator_a) * patch.oscillator_mix;
        let mut raw = pitched;
        if patch.sub_level > 0.0 {
            raw += self.sub.process() * patch.sub_level;
        }
        if patch.noise_level > 0.0 {
            raw += self.noise() * patch.noise_level;
        }
        let raw = raw / (1.0 + patch.sub_level + patch.noise_level);
        self.filter.process(raw * level * self.velocity)
    }

    #[inline]
    fn noise(&mut self) -> f32 {
        let mut value = self.noise_state;
        value ^= value << 13;
        value ^= value >> 7;
        value ^= value << 17;
        self.noise_state = value;
        ((value >> 40) as f32 * (1.0 / 8_388_607.5)) - 1.0
    }

    fn set_frequencies(&mut self, patch: &Patch) {
        let frequency = pitch_to_hertz(self.pitch);
        let count = usize::from(patch.unison_voices);
        let center = (count as f32 - 1.0) * 0.5;
        for index in 0..count {
            let cents = (index as f32 - center) * patch.unison_detune_cents;
            let ratio = 2.0_f32.powf(cents / 1_200.0);
            self.oscillator_a[index].set_frequency(frequency * ratio);
            let secondary = 2.0_f32.powf((cents + patch.oscillator_b_detune_cents) / 1_200.0);
            self.oscillator_b[index].set_frequency(frequency * secondary);
        }
        self.sub.set_frequency(frequency * 0.5);
    }

    fn reset_sources(&mut self) {
        for oscillator in &mut self.oscillator_a {
            oscillator.reset();
        }
        for oscillator in &mut self.oscillator_b {
            oscillator.reset();
        }
        self.sub.reset();
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
        let patch = sanitize_patch(patch, self.sample_rate);
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
            for oscillator in &mut voice.oscillator_a {
                oscillator.set_shape(patch.shape);
            }
            for oscillator in &mut voice.oscillator_b {
                oscillator.set_shape(patch.shape_b);
            }
            voice.envelope.set_settings(patch.envelope);
            voice.filter.set_coefficients(coefficients);
            voice.set_frequencies(&patch);
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
        voice.set_frequencies(&self.patch);
        // A voice taken from another note starts its waveform afresh so
        // the new note does not inherit a click from the old one.
        voice.reset_sources();
        voice.noise_state = (u64::from(pitch) + 1)
            .wrapping_mul(0x9e3779b97f4a7c15)
            .wrapping_add(counter)
            .max(1);
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
        self.counter = 0;
        for voice in &mut self.voices {
            voice.active = false;
            voice.envelope.reset();
            voice.reset_sources();
            voice.noise_state = 1;
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
                sum += voice.process(&self.patch);
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

fn sanitize_patch(patch: Patch, sample_rate: f32) -> Patch {
    let finite = |value: f32, fallback: f32| {
        if value.is_finite() { value } else { fallback }
    };
    Patch {
        oscillator_mix: crate::dsp::clamp(patch.oscillator_mix, 0.0, 1.0),
        oscillator_b_detune_cents: finite(patch.oscillator_b_detune_cents, 0.0)
            .clamp(-2_400.0, 2_400.0),
        sub_level: crate::dsp::clamp(patch.sub_level, 0.0, 1.0),
        noise_level: crate::dsp::clamp(patch.noise_level, 0.0, 1.0),
        unison_voices: patch.unison_voices.clamp(1, MAX_UNISON as u8),
        unison_detune_cents: finite(patch.unison_detune_cents, 0.0).clamp(0.0, 100.0),
        envelope: Settings {
            attack: finite(patch.envelope.attack, 0.0).clamp(0.0, 60.0),
            decay: finite(patch.envelope.decay, 0.0).clamp(0.0, 60.0),
            sustain: crate::dsp::clamp(patch.envelope.sustain, 0.0, 1.0),
            release: finite(patch.envelope.release, 0.0).clamp(0.0, 60.0),
        },
        cutoff: finite(patch.cutoff, 20.0).clamp(20.0, 20_000.0_f32.min(sample_rate * 0.4975)),
        resonance: finite(patch.resonance, 0.707).clamp(0.001, 100.0),
        level_db: if patch.level_db == f32::NEG_INFINITY {
            patch.level_db
        } else {
            finite(patch.level_db, -120.0).clamp(-120.0, 6.0)
        },
        ..patch
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
    fn dual_oscillator_sub_noise_and_unison_change_the_sound() {
        let render = |patch: Patch| {
            let mut bank = VoiceBank::new(patch, RATE);
            bank.note_on(60, 110);
            let mut output = [[0.0_f32; 2]; 2_048];
            bank.render(&mut output, 0.0);
            output
        };
        let plain = render(Patch {
            oscillator_mix: 0.0,
            sub_level: 0.0,
            noise_level: 0.0,
            unison_voices: 1,
            ..Patch::default()
        });
        let layered = render(Patch {
            oscillator_mix: 0.55,
            sub_level: 0.4,
            noise_level: 0.08,
            unison_voices: 4,
            unison_detune_cents: 18.0,
            ..Patch::default()
        });
        assert_ne!(plain, layered);
        assert!(peak(&layered) > 0.001);
        assert!(layered.iter().flatten().all(|sample| sample.is_finite()));
    }

    #[test]
    fn noise_and_unison_render_deterministically() {
        let patch = Patch {
            noise_level: 0.25,
            unison_voices: 4,
            unison_detune_cents: 20.0,
            ..Patch::default()
        };
        let mut bank = VoiceBank::new(patch, RATE);
        let mut first = [[0.0_f32; 2]; 1_024];
        bank.note_on(64, 100);
        bank.render(&mut first, 0.0);
        bank.reset();
        let mut second = [[0.0_f32; 2]; 1_024];
        bank.note_on(64, 100);
        bank.render(&mut second, 0.0);
        assert_eq!(first, second);
    }

    #[test]
    fn patch_values_are_bounded_before_rendering() {
        let mut bank = VoiceBank::new(
            Patch {
                oscillator_mix: f32::NAN,
                oscillator_b_detune_cents: f32::INFINITY,
                sub_level: -1.0,
                noise_level: 2.0,
                unison_voices: 255,
                unison_detune_cents: f32::NAN,
                cutoff: f32::INFINITY,
                resonance: -1.0,
                level_db: f32::NAN,
                ..Patch::default()
            },
            RATE,
        );
        let clean = bank.patch();
        assert_eq!(clean.oscillator_mix, 0.0);
        assert_eq!(clean.oscillator_b_detune_cents, 0.0);
        assert_eq!(clean.sub_level, 0.0);
        assert_eq!(clean.noise_level, 1.0);
        assert_eq!(clean.unison_voices, MAX_UNISON as u8);
        assert_eq!(clean.unison_detune_cents, 0.0);
        assert_eq!(clean.cutoff, 20.0);
        assert_eq!(clean.resonance, 0.001);
        assert_eq!(clean.level_db, -120.0);
        bank.note_on(60, 127);
        let mut output = [[0.0_f32; 2]; 2_048];
        bank.render(&mut output, 0.0);
        assert!(output.iter().flatten().all(|sample| sample.is_finite()));
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
