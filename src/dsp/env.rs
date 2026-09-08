//! Attack, decay, sustain, release envelope.
//!
//! Stage lengths are in seconds and converted to per-sample coefficients.
//! The curves are exponential, which is what an analog envelope does and
//! what a listener expects; the attack overshoots its target internally so
//! it arrives in the stated time rather than approaching forever.

/// Where an envelope is in its cycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// Producing nothing, waiting for a note.
    Idle,
    /// Rising to one.
    Attack,
    /// Falling to the sustain level.
    Decay,
    /// Holding the sustain level while the note is held.
    Sustain,
    /// Falling to zero after the note ends.
    Release,
}

/// Stage lengths and level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    /// Seconds to rise from zero to one.
    pub attack: f32,
    /// Seconds to fall from one to the sustain level.
    pub decay: f32,
    /// Level held while the note is on, 0 to 1.
    pub sustain: f32,
    /// Seconds to fall from the sustain level to zero.
    pub release: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            attack: 0.005,
            decay: 0.1,
            sustain: 0.7,
            release: 0.2,
        }
    }
}

/// One envelope generator.
#[derive(Clone, Copy, Debug)]
pub struct Envelope {
    settings: Settings,
    sample_rate: f32,
    stage: Stage,
    value: f32,
    attack_coefficient: f32,
    decay_coefficient: f32,
    release_coefficient: f32,
    // Asymptote of the stage in progress, set on entry from the level the
    // stage starts at so its length matches the configured time.
    asymptote: f32,
}

/// Curves reach their target when they come this close, keeping stage
/// lengths finite.
const TARGET_RATIO: f32 = 0.0001;

fn coefficient(seconds: f32, sample_rate: f32) -> f32 {
    if seconds <= 0.0 || sample_rate <= 0.0 {
        0.0
    } else {
        (-1.0 / (seconds * sample_rate) * (1.0 / TARGET_RATIO).ln()).exp()
    }
}

impl Envelope {
    /// An idle envelope.
    #[must_use]
    pub fn new(settings: Settings, sample_rate: f32) -> Self {
        let mut envelope = Self {
            settings,
            sample_rate: if sample_rate > 0.0 {
                sample_rate
            } else {
                48_000.0
            },
            stage: Stage::Idle,
            value: 0.0,
            attack_coefficient: 0.0,
            decay_coefficient: 0.0,
            release_coefficient: 0.0,
            asymptote: 0.0,
        };
        envelope.set_settings(settings);
        envelope
    }

    /// Replaces the settings. A stage in progress keeps its level and
    /// continues at the new rate.
    pub fn set_settings(&mut self, settings: Settings) {
        let clean = Settings {
            attack: settings.attack.max(0.0),
            decay: settings.decay.max(0.0),
            sustain: super::clamp(settings.sustain, 0.0, 1.0),
            release: settings.release.max(0.0),
        };
        self.settings = clean;
        self.attack_coefficient = coefficient(clean.attack, self.sample_rate);
        self.decay_coefficient = coefficient(clean.decay, self.sample_rate);
        self.release_coefficient = coefficient(clean.release, self.sample_rate);
    }

    /// Current settings.
    #[inline]
    #[must_use]
    pub const fn settings(&self) -> Settings {
        self.settings
    }

    /// Stage in progress.
    #[inline]
    #[must_use]
    pub const fn stage(&self) -> Stage {
        self.stage
    }

    /// Level last produced.
    #[inline]
    #[must_use]
    pub const fn value(&self) -> f32 {
        self.value
    }

    /// True while the envelope still produces sound.
    #[inline]
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Starts a note. Retriggering while sounding continues from the
    /// current level rather than clicking back to zero.
    #[inline]
    pub fn note_on(&mut self) {
        self.stage = Stage::Attack;
        self.asymptote = 1.0 + TARGET_RATIO * (1.0 - self.value).max(TARGET_RATIO);
    }

    /// Ends a note, moving to the release stage.
    #[inline]
    pub fn note_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
            self.asymptote = -TARGET_RATIO * self.value.max(TARGET_RATIO);
        }
    }

    /// Silences the envelope at once.
    #[inline]
    pub fn reset(&mut self) {
        self.stage = Stage::Idle;
        self.value = 0.0;
    }

    /// Produces the next level and advances the stage.
    #[inline]
    pub fn process(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => self.value = 0.0,
            Stage::Attack => {
                // The curve aims past one so it crosses in the stated time.
                self.value =
                    self.asymptote + (self.value - self.asymptote) * self.attack_coefficient;
                if self.value >= 1.0 || self.attack_coefficient == 0.0 {
                    self.value = 1.0;
                    if self.settings.sustain >= 1.0 {
                        self.stage = Stage::Sustain;
                    } else {
                        self.stage = Stage::Decay;
                        self.asymptote = self.settings.sustain
                            - TARGET_RATIO * (1.0 - self.settings.sustain).max(TARGET_RATIO);
                    }
                }
            }
            Stage::Decay => {
                self.value =
                    self.asymptote + (self.value - self.asymptote) * self.decay_coefficient;
                if self.value <= self.settings.sustain || self.decay_coefficient == 0.0 {
                    self.value = self.settings.sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.value = self.settings.sustain,
            Stage::Release => {
                self.value =
                    self.asymptote + (self.value - self.asymptote) * self.release_coefficient;
                if self.value <= 0.0 || self.release_coefficient == 0.0 {
                    self.value = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.value = super::flush_denormal(self.value);
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    fn settings(attack: f32, decay: f32, sustain: f32, release: f32) -> Settings {
        Settings {
            attack,
            decay,
            sustain,
            release,
        }
    }

    #[test]
    fn idle_until_a_note_starts() {
        let mut envelope = Envelope::new(Settings::default(), RATE);
        assert_eq!(envelope.stage(), Stage::Idle);
        assert!(!envelope.is_active());
        for _ in 0..100 {
            assert_eq!(envelope.process(), 0.0);
        }
    }

    #[test]
    fn stages_run_in_order_with_the_stated_lengths() {
        let mut envelope = Envelope::new(settings(0.01, 0.02, 0.5, 0.03), RATE);
        envelope.note_on();
        let mut attack_samples = 0;
        while envelope.stage() == Stage::Attack {
            envelope.process();
            attack_samples += 1;
            assert!(attack_samples < 48_000);
        }
        // 10 ms at 48 kHz is 480 samples, within a few per cent.
        assert!(
            (attack_samples as f32 - 480.0).abs() < 30.0,
            "{attack_samples}"
        );
        assert_eq!(envelope.value(), 1.0);

        let mut decay_samples = 0;
        while envelope.stage() == Stage::Decay {
            envelope.process();
            decay_samples += 1;
            assert!(decay_samples < 48_000);
        }
        assert!(
            (decay_samples as f32 - 960.0).abs() < 60.0,
            "{decay_samples}"
        );
        assert!((envelope.value() - 0.5).abs() < 1e-6);

        for _ in 0..1_000 {
            assert!((envelope.process() - 0.5).abs() < 1e-6);
        }
        assert_eq!(envelope.stage(), Stage::Sustain);

        envelope.note_off();
        let mut release_samples = 0;
        while envelope.stage() == Stage::Release {
            envelope.process();
            release_samples += 1;
            assert!(release_samples < 48_000);
        }
        assert!(
            (release_samples as f32 - 1_440.0).abs() < 90.0,
            "{release_samples}"
        );
        assert_eq!(envelope.value(), 0.0);
        assert!(!envelope.is_active());
    }

    #[test]
    fn attack_is_monotonic_and_release_falls() {
        let mut envelope = Envelope::new(settings(0.05, 0.05, 0.6, 0.05), RATE);
        envelope.note_on();
        let mut previous = 0.0;
        while envelope.stage() == Stage::Attack {
            let value = envelope.process();
            assert!(value >= previous);
            previous = value;
        }
        while envelope.stage() != Stage::Sustain {
            envelope.process();
        }
        envelope.note_off();
        previous = envelope.value();
        while envelope.stage() == Stage::Release {
            let value = envelope.process();
            assert!(value <= previous + 1e-6, "{value} {previous}");
            previous = value;
        }
    }

    #[test]
    fn full_sustain_skips_decay() {
        let mut envelope = Envelope::new(settings(0.001, 0.5, 1.0, 0.01), RATE);
        envelope.note_on();
        while envelope.stage() == Stage::Attack {
            envelope.process();
        }
        assert_eq!(envelope.stage(), Stage::Sustain);
    }

    #[test]
    fn zero_sustain_reaches_silence_and_stays_in_decay() {
        let mut envelope = Envelope::new(settings(0.001, 0.005, 0.0, 0.01), RATE);
        envelope.note_on();
        for _ in 0..2_000 {
            envelope.process();
        }
        assert_eq!(envelope.stage(), Stage::Sustain);
        assert_eq!(envelope.value(), 0.0);
    }

    #[test]
    fn zero_length_stages_are_immediate() {
        let mut envelope = Envelope::new(settings(0.0, 0.0, 0.5, 0.0), RATE);
        envelope.note_on();
        assert_eq!(envelope.process(), 1.0);
        assert_eq!(envelope.process(), 0.5);
        envelope.note_off();
        assert_eq!(envelope.process(), 0.0);
        assert_eq!(envelope.stage(), Stage::Idle);
    }

    #[test]
    fn retrigger_continues_from_the_current_level() {
        let mut envelope = Envelope::new(settings(0.01, 0.01, 0.5, 0.5), RATE);
        envelope.note_on();
        while envelope.stage() != Stage::Sustain {
            envelope.process();
        }
        envelope.note_off();
        for _ in 0..100 {
            envelope.process();
        }
        let level = envelope.value();
        assert!(level > 0.0 && level < 0.5);
        envelope.note_on();
        assert_eq!(envelope.stage(), Stage::Attack);
        // No jump back to zero.
        assert!(envelope.process() >= level);
    }

    #[test]
    fn note_off_while_idle_does_nothing() {
        let mut envelope = Envelope::new(Settings::default(), RATE);
        envelope.note_off();
        assert_eq!(envelope.stage(), Stage::Idle);
    }

    #[test]
    fn settings_are_clamped() {
        let mut envelope = Envelope::new(settings(-1.0, -1.0, 5.0, -1.0), RATE);
        assert_eq!(envelope.settings().attack, 0.0);
        assert_eq!(envelope.settings().sustain, 1.0);
        envelope.set_settings(settings(0.1, 0.1, -3.0, 0.1));
        assert_eq!(envelope.settings().sustain, 0.0);
    }

    #[test]
    fn reset_silences_immediately() {
        let mut envelope = Envelope::new(Settings::default(), RATE);
        envelope.note_on();
        for _ in 0..100 {
            envelope.process();
        }
        envelope.reset();
        assert_eq!(envelope.value(), 0.0);
        assert_eq!(envelope.stage(), Stage::Idle);
    }

    #[test]
    fn level_never_leaves_the_unit_range() {
        let mut envelope = Envelope::new(settings(0.002, 0.003, 0.8, 0.004), RATE);
        for cycle in 0..20 {
            if cycle % 2 == 0 {
                envelope.note_on();
            } else {
                envelope.note_off();
            }
            for _ in 0..500 {
                let value = envelope.process();
                assert!((0.0..=1.0).contains(&value), "{value}");
            }
        }
    }
}
