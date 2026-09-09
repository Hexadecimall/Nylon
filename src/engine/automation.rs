//! Fixed-capacity scheduling for persistent mixer automation.

use crate::engine::schedule::Span;
use crate::mixer::{AutomationCurve, AutomationEvent, MAX_AUTOMATION_EVENTS, Parameter};

/// Largest number of lanes published to the audio thread.
pub const MAX_AUTOMATION_LANES: usize = 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub beat: f64,
    pub value: f32,
    pub curve: AutomationCurve,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub track: u16,
    pub parameter: Parameter,
    pub points: Vec<Point>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Timeline {
    lanes: Vec<Lane>,
}

impl Timeline {
    #[must_use]
    pub const fn new() -> Self {
        Self { lanes: Vec::new() }
    }

    /// Adds one prepared lane. Returns false at the fixed lane limit.
    pub fn add_lane(&mut self, lane: Lane) -> bool {
        if self.lanes.len() == MAX_AUTOMATION_LANES {
            return false;
        }
        self.lanes.push(lane);
        true
    }

    #[must_use]
    pub fn lanes(&self) -> &[Lane] {
        &self.lanes
    }

    /// Schedules all segment boundaries that affect one render block.
    pub fn schedule(&self, span: Span, events: &mut [AutomationEvent]) -> Scheduled {
        let mut scheduled = Scheduled::default();
        if span.frames == 0 || !span.start_beats.is_finite() {
            return scheduled;
        }
        if !span.is_moving() {
            for lane in &self.lanes {
                if let Some(value) = value_at(lane, span.start_beats) {
                    push_event(static_event(lane, 0, value), events, &mut scheduled);
                }
            }
            events[..scheduled.count].sort_by(|left, right| {
                left.track
                    .cmp(&right.track)
                    .then(parameter_order(left.parameter).cmp(&parameter_order(right.parameter)))
            });
            return scheduled;
        }
        for lane in &self.lanes {
            schedule_lane(lane, span, events, &mut scheduled);
        }
        events[..scheduled.count].sort_by(|left, right| {
            left.offset
                .cmp(&right.offset)
                .then(left.track.cmp(&right.track))
                .then(parameter_order(left.parameter).cmp(&parameter_order(right.parameter)))
        });
        scheduled
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Scheduled {
    pub count: usize,
    pub dropped: usize,
}

pub const EMPTY_EVENT: AutomationEvent = AutomationEvent {
    offset: 0,
    track: 0,
    parameter: Parameter::Volume,
    start_value: 0.0,
    end_value: 0.0,
    elapsed_frames: 0,
    total_frames: 0,
    curve: AutomationCurve::Step,
};

fn schedule_lane(
    lane: &Lane,
    span: Span,
    events: &mut [AutomationEvent],
    scheduled: &mut Scheduled,
) {
    if lane.points.is_empty() {
        return;
    }
    let start = span.start_beats;
    let end = span.end_beats();
    if let Some(index) = lane.points.iter().rposition(|point| point.beat <= start) {
        push_event(
            segment_event(lane, index, 0, start, span),
            events,
            scheduled,
        );
    } else {
        push_event(
            static_event(lane, 0, lane.points[0].value),
            events,
            scheduled,
        );
    }
    for (index, point) in lane.points.iter().enumerate() {
        if point.beat <= start {
            continue;
        }
        if point.beat >= end {
            break;
        }
        push_event(
            segment_event(lane, index, span.frame_of(point.beat), point.beat, span),
            events,
            scheduled,
        );
    }
}

fn static_event(lane: &Lane, offset: usize, value: f32) -> AutomationEvent {
    AutomationEvent {
        offset,
        track: lane.track,
        parameter: lane.parameter,
        start_value: value,
        end_value: value,
        elapsed_frames: 0,
        total_frames: 0,
        curve: AutomationCurve::Step,
    }
}

fn value_at(lane: &Lane, beat: f64) -> Option<f32> {
    let first = *lane.points.first()?;
    let Some(index) = lane.points.iter().rposition(|point| point.beat <= beat) else {
        return Some(first.value);
    };
    let point = lane.points[index];
    let Some(next) = lane.points.get(index + 1).copied() else {
        return Some(point.value);
    };
    if point.curve == AutomationCurve::Step {
        return Some(point.value);
    }
    let phase = ((beat - point.beat) / (next.beat - point.beat)).clamp(0.0, 1.0);
    let shaped = match point.curve {
        AutomationCurve::Step => 0.0,
        AutomationCurve::Linear => phase,
        AutomationCurve::Smooth => phase * phase * (3.0 - 2.0 * phase),
    } as f32;
    Some(point.value + (next.value - point.value) * shaped)
}

fn segment_event(
    lane: &Lane,
    index: usize,
    offset: usize,
    current_beat: f64,
    span: Span,
) -> AutomationEvent {
    let point = lane.points[index];
    let Some(next) = lane.points.get(index + 1).copied() else {
        return AutomationEvent {
            offset,
            track: lane.track,
            parameter: lane.parameter,
            start_value: point.value,
            end_value: point.value,
            elapsed_frames: 0,
            total_frames: 0,
            curve: AutomationCurve::Step,
        };
    };
    let frames_per_beat = span.frames as f64 / span.length_beats;
    let total = frames_from_beats(next.beat - point.beat, frames_per_beat);
    let elapsed = frames_from_beats(current_beat - point.beat, frames_per_beat).min(total);
    AutomationEvent {
        offset,
        track: lane.track,
        parameter: lane.parameter,
        start_value: point.value,
        end_value: next.value,
        elapsed_frames: elapsed,
        total_frames: total,
        curve: point.curve,
    }
}

fn frames_from_beats(beats: f64, frames_per_beat: f64) -> u64 {
    let frames = beats * frames_per_beat;
    if !frames.is_finite() || frames <= 0.0 {
        0
    } else if frames >= u64::MAX as f64 {
        u64::MAX
    } else {
        frames.round() as u64
    }
}

fn push_event(event: AutomationEvent, events: &mut [AutomationEvent], scheduled: &mut Scheduled) {
    if scheduled.count < events.len().min(MAX_AUTOMATION_EVENTS) {
        events[scheduled.count] = event;
        scheduled.count += 1;
    } else {
        scheduled.dropped += 1;
    }
}

const fn parameter_order(parameter: Parameter) -> u8 {
    match parameter {
        Parameter::Volume => 0,
        Parameter::Pan => 1,
        Parameter::Mute => 2,
        Parameter::Solo => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lane(curve: AutomationCurve) -> Lane {
        Lane {
            track: 2,
            parameter: Parameter::Volume,
            points: vec![
                Point {
                    beat: 0.0,
                    value: -12.0,
                    curve,
                },
                Point {
                    beat: 4.0,
                    value: 0.0,
                    curve: AutomationCurve::Step,
                },
            ],
        }
    }

    #[test]
    fn a_block_inside_a_segment_keeps_the_original_ramp_phase() {
        let mut timeline = Timeline::new();
        assert!(timeline.add_lane(lane(AutomationCurve::Linear)));
        let mut events = [EMPTY_EVENT; 8];
        let result = timeline.schedule(
            Span {
                start_beats: 2.0,
                length_beats: 1.0,
                frames: 100,
            },
            &mut events,
        );
        assert_eq!(result.count, 1);
        assert_eq!(events[0].offset, 0);
        assert_eq!(events[0].elapsed_frames, 200);
        assert_eq!(events[0].total_frames, 400);
    }

    #[test]
    fn boundaries_inside_the_block_land_on_the_matching_frame() {
        let mut timeline = Timeline::new();
        assert!(timeline.add_lane(Lane {
            track: 0,
            parameter: Parameter::Pan,
            points: vec![
                Point {
                    beat: 1.0,
                    value: -1.0,
                    curve: AutomationCurve::Linear,
                },
                Point {
                    beat: 1.5,
                    value: 1.0,
                    curve: AutomationCurve::Step,
                },
            ],
        }));
        let mut events = [EMPTY_EVENT; 8];
        let result = timeline.schedule(
            Span {
                start_beats: 0.5,
                length_beats: 2.0,
                frames: 200,
            },
            &mut events,
        );
        assert_eq!(result.count, 3);
        assert_eq!(events[0].offset, 0);
        assert_eq!(events[1].offset, 50);
        assert_eq!(events[2].offset, 100);
    }

    #[test]
    fn capacity_loss_is_reported() {
        let mut timeline = Timeline::new();
        assert!(timeline.add_lane(lane(AutomationCurve::Step)));
        let result = timeline.schedule(
            Span {
                start_beats: 0.0,
                length_beats: 8.0,
                frames: 128,
            },
            &mut [],
        );
        assert_eq!(result.dropped, 2);
    }

    #[test]
    fn a_stopped_block_holds_the_value_at_the_playhead() {
        let mut timeline = Timeline::new();
        assert!(timeline.add_lane(lane(AutomationCurve::Linear)));
        let mut events = [EMPTY_EVENT; 2];
        let result = timeline.schedule(
            Span {
                start_beats: 2.0,
                length_beats: 0.0,
                frames: 128,
            },
            &mut events,
        );
        assert_eq!(result.count, 1);
        assert_eq!(events[0].start_value, -6.0);
        assert_eq!(events[0].end_value, -6.0);
        assert_eq!(events[0].curve, AutomationCurve::Step);
    }
}
