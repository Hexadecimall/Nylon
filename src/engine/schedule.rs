//! Turning notes on a timeline into events on a block.
//!
//! A clip stores notes in beats. A block covers a span of beats, and the
//! instrument needs to be told which notes begin and end inside that span
//! and at which sample. The scheduler does that conversion.
//!
//! Notes are kept sorted by start, so a block only looks at the ones that
//! could fall inside it rather than scanning the whole clip. Nothing here
//! allocates: the caller supplies the note storage and the event buffer.

use crate::mixer::MAX_FRAMES;

/// Note events one block can carry. A block asking for more than this
/// loses the rest, which is reported so a caller can notice.
pub const MAX_NOTE_EVENTS: usize = 256;
/// Loop repetitions one block may inspect before reporting overflow.
const MAX_LOOP_CYCLES_PER_BLOCK: i64 = 256;

/// A note on a clip's timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScheduledNote {
    /// Beat the note starts on, from the clip's start.
    pub start_beats: f64,
    /// Beats the note lasts.
    pub length_beats: f64,
    /// Note number.
    pub pitch: u8,
    /// How hard the note is struck, 1 to 127.
    pub velocity: u8,
}

impl ScheduledNote {
    /// Beat the note ends on.
    #[must_use]
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }

    /// Whether the note could ever sound.
    #[must_use]
    pub fn is_playable(&self) -> bool {
        self.start_beats.is_finite()
            && self.length_beats.is_finite()
            && self.start_beats >= 0.0
            && self.length_beats > 0.0
            && self.velocity > 0
    }
}

/// What a scheduled event asks the instrument to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteAction {
    /// Start the note.
    On,
    /// Release it.
    Off,
}

/// A note event placed on a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteEvent {
    /// Frame within the block the event lands on.
    pub offset: usize,
    /// Note number.
    pub pitch: u8,
    /// Velocity, meaningful for a note starting.
    pub velocity: u8,
    /// What to do.
    pub action: NoteAction,
}

/// Result of scheduling one block.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Scheduled {
    /// Events written to the buffer, in ascending offset order.
    pub count: usize,
    /// Events that did not fit. Nonzero means notes were lost.
    pub dropped: usize,
}

/// The span of musical time a block covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Span {
    /// Beat the block starts on.
    pub start_beats: f64,
    /// Beats the block covers. Zero means the transport is stopped.
    pub length_beats: f64,
    /// Frames in the block.
    pub frames: usize,
}

impl Span {
    /// Beat just past the block.
    #[must_use]
    pub fn end_beats(&self) -> f64 {
        self.start_beats + self.length_beats
    }

    /// Whether the block covers any musical time.
    #[must_use]
    pub fn is_moving(&self) -> bool {
        self.length_beats > 0.0 && self.frames > 0 && self.start_beats.is_finite()
    }

    /// Frame within the block a beat falls on, clamped into the block.
    #[must_use]
    pub fn frame_of(&self, beat: f64) -> usize {
        if !self.is_moving() {
            return 0;
        }
        let position = (beat - self.start_beats) / self.length_beats;
        let frame = position * self.frames as f64;
        if frame <= 0.0 {
            0
        } else if frame >= self.frames as f64 {
            self.frames - 1
        } else {
            frame as usize
        }
    }
}

/// Places the note starts and ends that fall inside `span` into `events`.
///
/// Notes must be sorted by start beat; [`sort_notes`] does that. Events
/// come out in ascending offset order, with a note's end never placed
/// before its start in the same block.
///
/// A note whose start and end both fall in the same block produces both
/// events. A note running past the block produces only its start, and its
/// end arrives in whichever later block contains it.
pub fn schedule_block(notes: &[ScheduledNote], span: Span, events: &mut [NoteEvent]) -> Scheduled {
    let mut result = Scheduled::default();
    if !span.is_moving() || span.frames > MAX_FRAMES {
        return result;
    }
    let start = span.start_beats;
    let end = span.end_beats();

    let mut push = |event: NoteEvent, result: &mut Scheduled| {
        if result.count < events.len() {
            events[result.count] = event;
            result.count += 1;
        } else {
            result.dropped += 1;
        }
    };

    for note in notes {
        if !note.is_playable() {
            continue;
        }
        // Notes are sorted, so once one starts past the block the rest do
        // too; their ends are handled by the pass below.
        if note.start_beats >= end && note.end_beats() >= end {
            break;
        }
        let starts_here = note.start_beats >= start && note.start_beats < end;
        let ends_here = note.end_beats() >= start && note.end_beats() < end;
        if starts_here {
            push(
                NoteEvent {
                    offset: span.frame_of(note.start_beats),
                    pitch: note.pitch,
                    velocity: note.velocity,
                    action: NoteAction::On,
                },
                &mut result,
            );
        }
        if ends_here {
            // A note shorter than one frame still gets its end after its
            // start, so the instrument does not release a note it has not
            // been given.
            let mut offset = span.frame_of(note.end_beats());
            if starts_here {
                offset = offset.max(span.frame_of(note.start_beats));
            }
            push(
                NoteEvent {
                    offset,
                    pitch: note.pitch,
                    velocity: 0,
                    action: NoteAction::Off,
                },
                &mut result,
            );
        }
    }

    let written = result.count;
    sort_note_events(&mut events[..written]);
    result
}

/// Places repeated clip notes into one transport block.
///
/// `launch_beats` is the transport position where the first loop begins.
/// Notes are relative to the clip timeline and only notes beginning inside
/// the selected loop participate.
pub fn schedule_looped_block(
    notes: &[ScheduledNote],
    loop_start_beats: f64,
    loop_length_beats: f64,
    launch_beats: f64,
    span: Span,
    events: &mut [NoteEvent],
) -> Scheduled {
    let mut result = Scheduled::default();
    if !span.is_moving()
        || span.frames > MAX_FRAMES
        || !loop_start_beats.is_finite()
        || loop_start_beats < 0.0
        || !loop_length_beats.is_finite()
        || loop_length_beats <= 0.0
        || !launch_beats.is_finite()
        || launch_beats < 0.0
    {
        return result;
    }
    let block_start = span.start_beats;
    let block_end = span.end_beats();
    if block_end <= launch_beats {
        return result;
    }
    let first_cycle =
        (((block_start - launch_beats) / loop_length_beats).floor() as i64 - 1).max(0);
    let last_cycle = ((block_end - launch_beats) / loop_length_beats).floor() as i64;
    let loop_end = loop_start_beats + loop_length_beats;
    if !loop_end.is_finite() || last_cycle < first_cycle {
        return result;
    }
    let capped_last_cycle = last_cycle.min(
        first_cycle
            .saturating_add(MAX_LOOP_CYCLES_PER_BLOCK)
            .saturating_sub(1),
    );

    for cycle in first_cycle..=capped_last_cycle {
        let cycle_start = launch_beats + cycle as f64 * loop_length_beats;
        for note in notes {
            if !note.is_playable()
                || note.start_beats < loop_start_beats
                || note.start_beats >= loop_end
            {
                continue;
            }
            let relative = note.start_beats - loop_start_beats;
            let start = cycle_start + relative;
            let available = loop_length_beats - relative;
            let end = start + note.length_beats.min(available);
            if start >= launch_beats && start >= block_start && start < block_end {
                push_note_event(
                    NoteEvent {
                        offset: span.frame_of(start),
                        pitch: note.pitch,
                        velocity: note.velocity,
                        action: NoteAction::On,
                    },
                    events,
                    &mut result,
                );
            }
            if end > launch_beats && end >= block_start && end < block_end {
                push_note_event(
                    NoteEvent {
                        offset: span.frame_of(end),
                        pitch: note.pitch,
                        velocity: 0,
                        action: NoteAction::Off,
                    },
                    events,
                    &mut result,
                );
            }
        }
    }
    if capped_last_cycle < last_cycle {
        result.dropped = result.dropped.saturating_add(1);
    }
    sort_note_events(&mut events[..result.count]);
    result
}

fn push_note_event(event: NoteEvent, events: &mut [NoteEvent], result: &mut Scheduled) {
    if result.count < events.len() {
        events[result.count] = event;
        result.count += 1;
    } else {
        result.dropped += 1;
    }
}

fn sort_note_events(events: &mut [NoteEvent]) {
    // Stable insertion sorting needs no scratch allocation. Equal offsets
    // keep generation order: a short note keeps On before Off, while a loop
    // edge keeps the preceding cycle's Off before the next cycle's On.
    for index in 1..events.len() {
        let mut position = index;
        while position > 0 && events[position - 1].offset > events[position].offset {
            events.swap(position - 1, position);
            position -= 1;
        }
    }
}

/// Sorts notes by start beat, which [`schedule_block`] relies on.
pub fn sort_notes(notes: &mut [ScheduledNote]) {
    notes.sort_by(|left, right| {
        left.start_beats
            .partial_cmp(&right.start_beats)
            .unwrap_or(core::cmp::Ordering::Equal)
    });
}

/// Notes sounding at `beat`, for starting playback in the middle of a
/// clip. Writes at most `out.len()` pitches and returns how many.
pub fn notes_sounding_at(notes: &[ScheduledNote], beat: f64, out: &mut [ScheduledNote]) -> usize {
    let mut count = 0;
    for note in notes {
        if count == out.len() {
            break;
        }
        if note.is_playable() && note.start_beats <= beat && note.end_beats() > beat {
            out[count] = *note;
            count += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(start: f64, length: f64, pitch: u8) -> ScheduledNote {
        ScheduledNote {
            start_beats: start,
            length_beats: length,
            pitch,
            velocity: 100,
        }
    }

    /// One beat over 480 frames, so a beat is easy to divide.
    fn span(start: f64, length: f64) -> Span {
        Span {
            start_beats: start,
            length_beats: length,
            frames: 480,
        }
    }

    #[test]
    fn a_stopped_transport_schedules_nothing() {
        let notes = [note(0.0, 1.0, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let stopped = Span {
            start_beats: 0.0,
            length_beats: 0.0,
            frames: 480,
        };
        assert_eq!(schedule_block(&notes, stopped, &mut events).count, 0);
        let empty = Span {
            start_beats: 0.0,
            length_beats: 1.0,
            frames: 0,
        };
        assert_eq!(schedule_block(&notes, empty, &mut events).count, 0);
    }

    #[test]
    fn a_note_starting_in_the_block_lands_on_the_right_frame() {
        let notes = [note(0.5, 2.0, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        // The block covers beats 0 to 1 over 480 frames, so half a beat in
        // is frame 240.
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 1);
        assert_eq!(result.dropped, 0);
        assert_eq!(events[0].offset, 240);
        assert_eq!(events[0].pitch, 60);
        assert_eq!(events[0].velocity, 100);
        assert_eq!(events[0].action, NoteAction::On);
    }

    #[test]
    fn a_note_contained_in_one_block_produces_both_events_in_order() {
        let notes = [note(0.25, 0.25, 64)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 2);
        assert_eq!(events[0].action, NoteAction::On);
        assert_eq!(events[0].offset, 120);
        assert_eq!(events[1].action, NoteAction::Off);
        assert_eq!(events[1].offset, 240);
        assert!(events[0].offset <= events[1].offset);
    }

    #[test]
    fn a_note_crossing_the_block_edge_starts_now_and_ends_later() {
        let notes = [note(0.5, 1.0, 67)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let first = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(first.count, 1);
        assert_eq!(events[0].action, NoteAction::On);

        let second = schedule_block(&notes, span(1.0, 1.0), &mut events);
        assert_eq!(second.count, 1);
        assert_eq!(events[0].action, NoteAction::Off);
        assert_eq!(events[0].pitch, 67);
        assert_eq!(events[0].offset, 240);
    }

    #[test]
    fn a_note_ending_on_a_block_boundary_is_released_in_the_next_block() {
        let notes = [note(0.5, 0.5, 67)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let first = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(first.count, 1);
        assert_eq!(events[0].action, NoteAction::On);
        let second = schedule_block(&notes, span(1.0, 1.0), &mut events);
        assert_eq!(second.count, 1);
        assert_eq!(events[0].action, NoteAction::Off);
        assert_eq!(events[0].offset, 0);
    }

    #[test]
    fn a_note_entirely_before_or_after_the_block_is_skipped() {
        let notes = [note(0.0, 0.25, 60), note(8.0, 1.0, 72)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_block(&notes, span(4.0, 1.0), &mut events);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn events_come_out_sorted_by_offset() {
        let mut notes = [
            note(0.75, 0.1, 72),
            note(0.1, 0.1, 60),
            note(0.5, 0.1, 67),
            note(0.25, 0.1, 64),
        ];
        sort_notes(&mut notes);
        assert_eq!(notes[0].pitch, 60);
        assert_eq!(notes[3].pitch, 72);

        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 16];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 8);
        let mut previous = 0;
        for event in &events[..result.count] {
            assert!(event.offset >= previous, "{event:?} after {previous}");
            previous = event.offset;
        }
    }

    #[test]
    fn a_note_shorter_than_a_frame_still_starts_before_it_ends() {
        // A note one thousandth of a beat long, well under a frame.
        let notes = [note(0.5, 0.000_1, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 2);
        assert_eq!(events[0].action, NoteAction::On);
        assert_eq!(events[1].action, NoteAction::Off);
        assert!(events[0].offset <= events[1].offset);
    }

    #[test]
    fn unplayable_notes_are_ignored() {
        let notes = [
            ScheduledNote {
                start_beats: 0.1,
                length_beats: 0.0,
                pitch: 60,
                velocity: 100,
            },
            ScheduledNote {
                start_beats: 0.2,
                length_beats: -1.0,
                pitch: 61,
                velocity: 100,
            },
            ScheduledNote {
                start_beats: -1.0,
                length_beats: 1.0,
                pitch: 62,
                velocity: 100,
            },
            ScheduledNote {
                start_beats: f64::NAN,
                length_beats: 1.0,
                pitch: 63,
                velocity: 100,
            },
            ScheduledNote {
                start_beats: 0.3,
                length_beats: 0.1,
                pitch: 64,
                velocity: 0,
            },
        ];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 16];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn a_full_buffer_reports_what_it_dropped() {
        let mut notes: Vec<ScheduledNote> = (0..20)
            .map(|index| note(f64::from(index) * 0.04, 0.01, 60 + index as u8))
            .collect();
        sort_notes(&mut notes);
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 5];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, 5);
        assert!(result.dropped > 0);
    }

    #[test]
    fn a_session_note_repeats_at_each_loop_boundary() {
        let notes = [note(0.0, 0.25, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 16];
        let result = schedule_looped_block(&notes, 0.0, 1.0, 2.0, span(2.0, 2.0), &mut events);
        assert_eq!(result.count, 4);
        assert_eq!(events[0].action, NoteAction::On);
        assert_eq!(events[0].offset, 0);
        assert_eq!(events[1].action, NoteAction::Off);
        assert_eq!(events[2].action, NoteAction::On);
        assert_eq!(events[2].offset, 240);
        assert_eq!(events[3].action, NoteAction::Off);
    }

    #[test]
    fn a_session_note_never_sounds_before_launch() {
        let notes = [note(0.0, 0.25, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_looped_block(&notes, 0.0, 1.0, 4.0, span(3.0, 1.0), &mut events);
        assert_eq!(result.count, 0);
    }

    #[test]
    fn a_session_note_is_cut_at_the_loop_edge() {
        let notes = [note(0.75, 1.0, 67)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_looped_block(&notes, 0.0, 1.0, 0.0, span(0.5, 1.5), &mut events);
        assert_eq!(result.count, 3);
        assert_eq!(events[0].action, NoteAction::On);
        assert_eq!(events[0].offset, 80);
        assert_eq!(events[1].action, NoteAction::Off);
        assert_eq!(events[1].offset, 160);
        assert_eq!(events[2].action, NoteAction::On);
        assert_eq!(events[2].offset, 400);
    }

    #[test]
    fn a_loop_edge_releases_before_retriggering_the_same_pitch() {
        let notes = [note(0.75, 0.25, 67), note(0.0, 0.25, 67)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let result = schedule_looped_block(&notes, 0.0, 1.0, 0.0, span(1.0, 0.5), &mut events);
        assert_eq!(result.count, 3);
        assert_eq!(events[0].offset, 0);
        assert_eq!(events[0].action, NoteAction::Off);
        assert_eq!(events[1].offset, 0);
        assert_eq!(events[1].action, NoteAction::On);
    }

    #[test]
    fn tiny_session_loops_have_a_fixed_work_limit() {
        let notes = [note(0.0, 0.000_001, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; MAX_NOTE_EVENTS];
        let result =
            schedule_looped_block(&notes, 0.0, 0.000_001, 0.0, span(0.0, 1.0), &mut events);
        assert_eq!(result.count, MAX_NOTE_EVENTS);
        assert!(result.dropped > 0);
    }

    #[test]
    fn frames_map_across_the_block() {
        let block = span(2.0, 1.0);
        assert_eq!(block.frame_of(2.0), 0);
        assert_eq!(block.frame_of(2.5), 240);
        assert_eq!(block.frame_of(2.999), 479);
        // Outside the block is clamped in.
        assert_eq!(block.frame_of(0.0), 0);
        assert_eq!(block.frame_of(100.0), 479);
        assert!(block.is_moving());
        assert!((block.end_beats() - 3.0).abs() < 1e-12);
    }

    #[test]
    fn a_block_longer_than_the_maximum_is_refused() {
        let notes = [note(0.0, 1.0, 60)];
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 8];
        let huge = Span {
            start_beats: 0.0,
            length_beats: 1.0,
            frames: MAX_FRAMES + 1,
        };
        assert_eq!(schedule_block(&notes, huge, &mut events).count, 0);
    }

    #[test]
    fn playing_a_run_of_blocks_gives_every_note_a_start_and_an_end() {
        let mut notes: Vec<ScheduledNote> = (0..16)
            .map(|index| note(f64::from(index) * 0.25, 0.2, 60 + index as u8))
            .collect();
        sort_notes(&mut notes);

        let mut starts = 0;
        let mut ends = 0;
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; MAX_NOTE_EVENTS];
        // Sixteen blocks of a quarter beat covers all four beats.
        for index in 0..20 {
            let block = Span {
                start_beats: f64::from(index) * 0.25,
                length_beats: 0.25,
                frames: 120,
            };
            let result = schedule_block(&notes, block, &mut events);
            for event in &events[..result.count] {
                match event.action {
                    NoteAction::On => starts += 1,
                    NoteAction::Off => ends += 1,
                }
            }
        }
        assert_eq!(starts, 16);
        assert_eq!(ends, 16);
    }

    #[test]
    fn notes_sounding_at_a_point_are_found() {
        let notes = [note(0.0, 4.0, 60), note(1.0, 0.5, 64), note(2.0, 4.0, 67)];
        let mut out = [note(0.0, 0.0, 0); 8];
        // At beat 1.25 the long note and the short one both sound.
        let count = notes_sounding_at(&notes, 1.25, &mut out);
        assert_eq!(count, 2);
        assert_eq!(out[0].pitch, 60);
        assert_eq!(out[1].pitch, 64);
        // At beat 3 the first and third sound.
        let count = notes_sounding_at(&notes, 3.0, &mut out);
        assert_eq!(count, 2);
        assert_eq!(out[0].pitch, 60);
        assert_eq!(out[1].pitch, 67);
        // Past everything, nothing sounds.
        assert_eq!(notes_sounding_at(&notes, 99.0, &mut out), 0);
        // A short destination takes what fits.
        let mut one = [note(0.0, 0.0, 0); 1];
        assert_eq!(notes_sounding_at(&notes, 1.25, &mut one), 1);
    }

    #[test]
    fn overlapping_notes_on_one_pitch_each_get_their_events() {
        let mut notes = [note(0.0, 0.6, 60), note(0.3, 0.6, 60)];
        sort_notes(&mut notes);
        let mut events = [NoteEvent {
            offset: 0,
            pitch: 0,
            velocity: 0,
            action: NoteAction::On,
        }; 16];
        let result = schedule_block(&notes, span(0.0, 1.0), &mut events);
        // Two starts and two ends, in offset order.
        assert_eq!(result.count, 4);
        let starts = events[..4]
            .iter()
            .filter(|event| event.action == NoteAction::On)
            .count();
        assert_eq!(starts, 2);
    }
}
