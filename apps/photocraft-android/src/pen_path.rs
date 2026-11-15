//! The pen's whole path. A digitizer reports 200 or more positions a second and Android hands
//! them over in batches, one `MotionEvent` with its history per batch; winit turns each event
//! into one touch move at its latest position, so a quick curve reached the brush as a few
//! dozen straight pieces (a large circle came out slightly polygonal). The Activity's pen hook
//! records every sample's position (`photocraft_tablet::android::take_pen_path`), and
//! [`PenPath`] puts them into the frame as touch moves, in order, before the touch router turns
//! them into pointer moves. Each move then also gets its own pen sample's pressure and tilt (the
//! stylus feed shares a frame's samples out over its moves).
//!
//! The pen hook is called before the event goes on to GameActivity and winit, so its samples are
//! never behind winit's moves, and usually a little ahead. Once a stroke's samples are known to
//! measure from the same corner as winit's positions, they *are* the stroke: a frame draws them
//! as soon as they arrive, whether or not winit's own move for them is in yet, and winit's moves
//! (the same positions, later) are dropped. That keeps a frame woken by the pen hook from
//! drawing nothing while the sample it was woken for waits for the next frame, and it keeps a
//! late move from pulling the stroke back to where it already was.
//!
//! When the two don't agree (winit's position far from every sample), the samples are not used
//! for that stroke and the moves stay as winit made them.

use egui::{Event, Pos2, TouchDeviceId, TouchId, TouchPhase, pos2};
use std::collections::VecDeque;

/// How far (points) winit's latest position may be from every pen sample around it.
pub const MAX_DRIFT: f32 = 24.0;
/// The most samples kept waiting for a frame (two seconds of a fast digitizer).
const MAX_PENDING: usize = 480;
/// Positions lately put into frames, for comparing with winit's (which trail by a few samples).
const RECENT: usize = 96;

/// The pen's contact, as winit reported its start.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Contact {
    device_id: TouchDeviceId,
    id: TouchId,
    force: Option<f32>,
}

/// Whether the samples and winit's positions measure from the same corner, for this stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Trust {
    /// Nothing to compare yet.
    Unknown,
    Agree,
    /// They don't: the stroke is drawn from winit's moves.
    Differ,
}

/// The pen's samples on their way into frames (see the module).
#[derive(Debug)]
pub struct PenPath {
    /// Sample positions (window pixels) not yet in a frame, oldest first.
    pending: Vec<(f32, f32)>,
    /// The pen touching the screen, from its touch-down to its lift.
    contact: Option<Contact>,
    /// The last positions (points) put into frames.
    recent: VecDeque<Pos2>,
    trust: Trust,
    /// The stroke (the pen hook's count of touch-downs) the waiting samples belong to.
    stroke: u64,
}

impl Default for PenPath {
    fn default() -> Self {
        Self { pending: Vec::new(), contact: None, recent: VecDeque::new(), trust: Trust::Unknown, stroke: 0 }
    }
}

/// A pen touch event's parts.
type PenEvent = (TouchDeviceId, TouchId, TouchPhase, Pos2, Option<f32>);

impl PenPath {
    /// Whether the pen is touching the screen (as far as the frames so far say).
    pub fn in_contact(&self) -> bool {
        self.contact.is_some()
    }

    /// Samples waiting for a frame.
    pub fn waiting(&self) -> usize {
        self.pending.len()
    }

    fn forget_stroke(&mut self) {
        self.pending.clear();
        self.recent.clear();
        self.contact = None;
        self.trust = Trust::Unknown;
    }

    /// Put the pen's samples into this frame's `events`: `fresh` are the positions (window
    /// pixels, `ppp` pixels per point) the pen hook recorded since the last frame, all of stroke
    /// number `stroke` (its count of touch-downs). Returns how many moves were put in.
    ///
    /// The samples replace the pen's own moves, where the first of them was; with no move in the
    /// frame they go just before the pen lifts, or at the end while it stays down.
    pub fn frame(&mut self, events: &mut Vec<Event>, stroke: u64, fresh: Vec<(f32, f32)>, ppp: f32, is_pen: impl Fn(TouchId) -> bool) -> usize {
        if stroke != self.stroke {
            // Samples of an earlier stroke still waiting (one the app never saw touch: the system
            // took it for a gesture) are not part of this one.
            self.pending.clear();
            self.stroke = stroke;
        }
        if !(ppp.is_finite() && ppp > 0.0) {
            // No scale to place the samples with: leave the frame alone.
            self.pending.clear();
            return 0;
        }
        let pen_event = |e: &Event| -> Option<PenEvent> {
            match e {
                Event::Touch { device_id, id, phase, pos, force } if is_pen(*id) => Some((*device_id, *id, *phase, *pos, *force)),
                _ => None,
            }
        };
        // A touch-down starts a new stroke: what comes before it in the frame (the end of the
        // last stroke) stays as it is, and the samples are the new stroke's.
        let from = events.iter().rposition(|e| matches!(pen_event(e), Some((_, _, TouchPhase::Start, ..)))).unwrap_or(0);
        if let Some((device_id, id, TouchPhase::Start, ..)) = events.get(from).and_then(pen_event) {
            self.recent.clear();
            self.trust = Trust::Unknown;
            self.contact = Some(Contact { device_id, id, force: None });
        }
        self.pending.extend(fresh.into_iter().filter(|(x, y)| x.is_finite() && y.is_finite()));
        if self.pending.len() > MAX_PENDING {
            let extra = self.pending.len() - MAX_PENDING;
            self.pending.drain(..extra);
        }
        // A pen already down when the app first sees it (no touch-down in any frame): its
        // first move stands in.
        if self.contact.is_none()
            && let Some((device_id, id, ..)) = events.iter().filter_map(pen_event).find(|e| e.2 == TouchPhase::Move)
        {
            self.contact = Some(Contact { device_id, id, force: None });
        }
        let Some(mut contact) = self.contact else {
            // No pen on the screen: samples (the first of a stroke whose touch-down isn't in
            // yet) wait, a bounded number of them.
            return 0;
        };
        let contact_id = contact.id;
        let mine = |e: &Event| pen_event(e).filter(|ev| ev.1 == contact_id);
        let stroke = &events[from..];
        if let Some(force) = stroke.iter().rev().find_map(|e| mine(e).map(|(.., force)| force)) {
            contact.force = force;
            self.contact = Some(contact);
        }
        let lift = stroke.iter().position(|e| matches!(mine(e), Some((_, _, TouchPhase::End | TouchPhase::Cancel, ..)))).map(|i| from + i);
        let first_move = stroke.iter().position(|e| matches!(mine(e), Some((_, _, TouchPhase::Move, ..)))).map(|i| from + i);
        // Do the samples and winit agree? Its latest position must be near one of them.
        let theirs = stroke.iter().rev().find_map(|e| mine(e).map(|(_, _, _, pos, _)| pos));
        if let Some(theirs) = theirs
            && self.trust != Trust::Differ
        {
            let near = |p: Pos2| p.distance(theirs) <= MAX_DRIFT;
            let known = !self.recent.is_empty() || !self.pending.is_empty();
            if self.recent.iter().copied().any(near) || self.pending.iter().any(|&(x, y)| near(pos2(x / ppp, y / ppp))) {
                self.trust = Trust::Agree;
            } else if known {
                log::warn!("pen samples are {MAX_DRIFT} points or more from the touch: not used for this stroke");
                self.trust = Trust::Differ;
            }
        }
        let mut put = 0;
        if self.trust == Trust::Agree {
            let at = first_move.or(lift).unwrap_or(events.len());
            let Contact { device_id, id, force } = contact;
            let moves: Vec<Event> =
                self.pending.drain(..).map(|(x, y)| Event::Touch { device_id, id, phase: TouchPhase::Move, pos: pos2(x / ppp, y / ppp), force }).collect();
            put = moves.len();
            for m in &moves {
                if let Event::Touch { pos, .. } = m {
                    if self.recent.len() >= RECENT {
                        self.recent.pop_front();
                    }
                    self.recent.push_back(*pos);
                }
            }
            let mut out = Vec::with_capacity(events.len() + moves.len());
            let mut moves = Some(moves);
            for (i, e) in events.drain(..).enumerate() {
                if i == at {
                    out.extend(moves.take().unwrap_or_default());
                }
                // The pen's own moves are among the samples: these ones, or ones a frame
                // already drew.
                if i >= from && matches!(mine(&e), Some((_, _, TouchPhase::Move, ..))) {
                    continue;
                }
                out.push(e);
            }
            out.extend(moves.take().unwrap_or_default());
            *events = out;
        } else if self.trust == Trust::Differ {
            self.pending.clear();
        }
        if lift.is_some() {
            self.forget_stroke();
        }
        put
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(id: u64, phase: TouchPhase, x: f32) -> Event {
        Event::Touch { device_id: TouchDeviceId(1), id: TouchId(id), phase, pos: pos2(x, 0.0), force: Some(0.5) }
    }

    fn xs(events: &[Event]) -> Vec<(u64, TouchPhase, f32)> {
        events
            .iter()
            .filter_map(|e| match e {
                Event::Touch { id, phase, pos, .. } => Some((id.0, *phase, pos.x)),
                _ => None,
            })
            .collect()
    }

    /// Ids from 100 up are pens.
    fn pen(id: TouchId) -> bool {
        id.0 >= 100
    }

    fn px(xs: &[f32]) -> Vec<(f32, f32)> {
        xs.iter().map(|&x| (x, 0.0)).collect()
    }

    /// A pen path with its touch-down already seen at `x` points (1 pixel per point).
    fn down_at(x: f32) -> PenPath {
        let mut path = PenPath::default();
        let mut events = vec![touch(100, TouchPhase::Start, x)];
        assert_eq!(path.frame(&mut events, 1, px(&[x]), 1.0, pen), 1, "the touch-down's own sample");
        path
    }

    #[test]
    fn every_sample_becomes_a_move_in_place_of_the_pens_own() {
        let mut path = down_at(10.0);
        let mut events = vec![touch(1, TouchPhase::Move, 5.0), touch(100, TouchPhase::Move, 30.0), Event::PointerGone, touch(100, TouchPhase::Move, 60.0)];
        // Pixels at 2 pixels per point: 20, 40, 60 points.
        assert_eq!(path.frame(&mut events, 1, px(&[40.0, 80.0, 120.0]), 2.0, pen), 3);
        assert_eq!(xs(&events), [(1, TouchPhase::Move, 5.0), (100, TouchPhase::Move, 20.0), (100, TouchPhase::Move, 40.0), (100, TouchPhase::Move, 60.0)]);
        assert!(events.contains(&Event::PointerGone), "other events stay");
    }

    #[test]
    fn the_last_samples_come_before_the_lift() {
        let mut path = down_at(5.0);
        let mut events = vec![touch(100, TouchPhase::End, 9.0)];
        path.frame(&mut events, 1, px(&[8.0]), 1.0, pen);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 8.0), (100, TouchPhase::End, 9.0)]);
        assert!(!path.in_contact());
    }

    #[test]
    fn samples_measured_from_elsewhere_are_not_used() {
        let mut path = PenPath::default();
        let mut events = vec![touch(100, TouchPhase::Start, 30.0)];
        // The samples are 100 points off: winit's events stay, for the whole stroke.
        assert_eq!(path.frame(&mut events, 1, px(&[130.0]), 1.0, pen), 0);
        assert_eq!(xs(&events), [(100, TouchPhase::Start, 30.0)]);
        let before = vec![touch(100, TouchPhase::Move, 40.0), touch(100, TouchPhase::Move, 50.0)];
        let mut events = before.clone();
        assert_eq!(path.frame(&mut events, 1, px(&[140.0, 150.0]), 1.0, pen), 0);
        assert_eq!(events, before);
        // And nothing of them is replayed later, when a sample happens to be near.
        let mut events = vec![touch(100, TouchPhase::Move, 150.0)];
        assert_eq!(path.frame(&mut events, 1, px(&[151.0]), 1.0, pen), 0);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 150.0)]);
        // The next stroke is judged afresh.
        let mut events = vec![touch(100, TouchPhase::End, 150.0)];
        path.frame(&mut events, 1, vec![], 1.0, pen);
        let mut events = vec![touch(100, TouchPhase::Start, 10.0)];
        assert_eq!(path.frame(&mut events, 1, px(&[10.0]), 1.0, pen), 1);
    }

    #[test]
    fn without_a_pen_touch_nothing_changes() {
        let mut path = PenPath::default();
        let before = vec![touch(1, TouchPhase::Move, 5.0)];
        let mut events = before.clone();
        assert_eq!(path.frame(&mut events, 1, px(&[1.0]), 1.0, pen), 0);
        assert_eq!(events, before, "a frame without the pen keeps the samples for one with it");
        assert_eq!(path.waiting(), 1);
        assert_eq!(path.frame(&mut events, 1, vec![], 1.0, pen), 0);
        assert_eq!(events, before);
    }

    #[test]
    fn a_frame_woken_by_the_pen_hook_draws_before_winits_move_arrives() {
        let mut path = down_at(10.0);
        // The hook's sample is here, winit's event for it is not: the stroke goes on anyway.
        let mut events = vec![];
        assert_eq!(path.frame(&mut events, 1, px(&[14.0, 18.0]), 1.0, pen), 2);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 14.0), (100, TouchPhase::Move, 18.0)]);
        // Then winit's moves for those samples arrive, with nothing new: they are dropped, so the
        // stroke doesn't go back to 14.
        let mut events = vec![touch(100, TouchPhase::Move, 14.0), touch(100, TouchPhase::Move, 18.0)];
        assert_eq!(path.frame(&mut events, 1, vec![], 1.0, pen), 0);
        assert_eq!(xs(&events), []);
    }

    #[test]
    fn a_stroke_never_goes_backwards_however_the_two_streams_interleave() {
        // 400 samples one point apart; winit reports each a varying number of frames later, in
        // batches, sometimes not at all. Whatever the interleaving, the moves a stroke gets are
        // the samples, each once, in order.
        for seed in 0..200u64 {
            let mut rng = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let mut next = |n: u64| {
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                rng % n
            };
            let mut path = PenPath::default();
            let mut drawn = Vec::new();
            let mut events = vec![touch(100, TouchPhase::Start, 0.0)];
            path.frame(&mut events, 1, px(&[0.0]), 1.0, pen);
            drawn.extend(xs(&events).into_iter().filter(|e| e.1 == TouchPhase::Move).map(|e| e.2));
            let (mut hook, mut winit) = (1usize, 1usize);
            while hook < 400 {
                // The hook is ahead of winit by up to 12 samples.
                let hook_to = (hook + next(6) as usize).min(400);
                let winit_to = (winit + next(8) as usize).min(hook_to).max(hook_to.saturating_sub(12)).max(winit);
                let fresh: Vec<f32> = (hook..hook_to).map(|i| i as f32).collect();
                // winit reports only the latest position of each batch.
                let mut events: Vec<Event> = if winit_to > winit && next(4) != 0 { vec![touch(100, TouchPhase::Move, (winit_to - 1) as f32)] } else { vec![] };
                path.frame(&mut events, 1, px(&fresh), 1.0, pen);
                drawn.extend(xs(&events).into_iter().map(|e| e.2));
                (hook, winit) = (hook_to, winit_to);
            }
            let mut events = vec![touch(100, TouchPhase::End, 399.0)];
            path.frame(&mut events, 1, vec![], 1.0, pen);
            drawn.extend(xs(&events).into_iter().filter(|e| e.1 == TouchPhase::Move).map(|e| e.2));
            let expected: Vec<f32> = (0..400).map(|i| i as f32).collect();
            assert_eq!(drawn, expected, "seed {seed}");
        }
    }

    #[test]
    fn a_quick_stroke_far_ahead_of_winit_still_agrees() {
        // 40 points a sample (a very fast stroke) and winit three samples behind: its position
        // is far from the newest sample but is one of the earlier ones.
        let mut path = down_at(0.0);
        let mut events = vec![];
        assert_eq!(path.frame(&mut events, 1, px(&[40.0, 80.0, 120.0, 160.0]), 1.0, pen), 4);
        let mut events = vec![touch(100, TouchPhase::Move, 40.0)];
        assert_eq!(path.frame(&mut events, 1, px(&[200.0]), 1.0, pen), 1);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 200.0)]);
    }

    #[test]
    fn a_new_touch_down_in_the_frame_of_the_last_lift_starts_afresh() {
        let mut path = down_at(10.0);
        // The first stroke's last move and lift, then the next stroke's touch-down and a move.
        let mut events = vec![
            touch(100, TouchPhase::Move, 12.0),
            touch(100, TouchPhase::End, 12.0),
            touch(100, TouchPhase::Start, 300.0),
            touch(100, TouchPhase::Move, 304.0),
        ];
        assert_eq!(path.frame(&mut events, 1, px(&[300.0, 302.0, 304.0]), 1.0, pen), 3);
        assert_eq!(
            xs(&events),
            [
                (100, TouchPhase::Move, 12.0),
                (100, TouchPhase::End, 12.0),
                (100, TouchPhase::Start, 300.0),
                (100, TouchPhase::Move, 300.0),
                (100, TouchPhase::Move, 302.0),
                (100, TouchPhase::Move, 304.0)
            ],
            "the new stroke's samples come after its touch-down"
        );
        assert!(path.in_contact());
    }

    #[test]
    fn a_cancelled_stroke_and_samples_without_a_scale_leave_nothing_behind() {
        let mut path = down_at(10.0);
        let mut events = vec![touch(100, TouchPhase::Cancel, 11.0)];
        path.frame(&mut events, 1, px(&[11.0]), 1.0, pen);
        assert!(!path.in_contact() && path.waiting() == 0);
        let mut events = vec![touch(100, TouchPhase::Move, 5.0)];
        assert_eq!(path.frame(&mut events, 1, px(&[5.0]), f32::NAN, pen), 0);
        assert_eq!(path.frame(&mut events, 1, px(&[5.0]), 0.0, pen), 0);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 5.0)]);
        // Non-finite samples are dropped, and a flood is bounded.
        let mut path = PenPath::default();
        let mut events = vec![];
        path.frame(&mut events, 1, vec![(f32::NAN, 0.0), (1.0, f32::INFINITY)], 1.0, pen);
        assert_eq!(path.waiting(), 0);
        path.frame(&mut events, 1, px(&vec![1.0; 5000]), 1.0, pen);
        assert_eq!(path.waiting(), MAX_PENDING);
    }

    #[test]
    fn samples_of_a_stroke_the_app_never_saw_are_not_drawn_into_the_next() {
        // A stroke the system kept for itself (a pen gesture): its samples arrive, no touch does.
        let mut path = PenPath::default();
        let mut events = vec![];
        assert_eq!(path.frame(&mut events, 1, px(&[500.0, 510.0, 520.0]), 1.0, pen), 0);
        assert_eq!(path.waiting(), 3);
        // The next stroke starts elsewhere: only its own samples are drawn.
        let mut events = vec![touch(100, TouchPhase::Start, 10.0)];
        assert_eq!(path.frame(&mut events, 2, px(&[10.0, 12.0]), 1.0, pen), 2);
        assert_eq!(xs(&events), [(100, TouchPhase::Start, 10.0), (100, TouchPhase::Move, 10.0), (100, TouchPhase::Move, 12.0)]);
        // Samples taken before the touch-down's frame, of the same stroke, are kept for it.
        let mut path = PenPath::default();
        let mut events = vec![];
        path.frame(&mut events, 7, px(&[10.0, 11.0]), 1.0, pen);
        let mut events = vec![touch(100, TouchPhase::Start, 10.0)];
        assert_eq!(path.frame(&mut events, 7, px(&[12.0]), 1.0, pen), 3);
    }

    #[test]
    fn a_pen_already_down_when_first_seen_is_followed_from_its_first_move() {
        let mut path = PenPath::default();
        let mut events = vec![touch(100, TouchPhase::Move, 20.0)];
        assert_eq!(path.frame(&mut events, 1, px(&[18.0, 20.0]), 1.0, pen), 2);
        assert_eq!(xs(&events), [(100, TouchPhase::Move, 18.0), (100, TouchPhase::Move, 20.0)]);
    }

    #[test]
    fn without_any_sample_winits_moves_are_the_stroke() {
        // The pen hook isn't working (the library's hook didn't load): nothing to compare with,
        // so the frame is as winit made it.
        let mut path = PenPath::default();
        let mut events = vec![touch(100, TouchPhase::Start, 1.0)];
        path.frame(&mut events, 1, vec![], 1.0, pen);
        let before = vec![touch(100, TouchPhase::Move, 2.0), touch(100, TouchPhase::Move, 3.0)];
        let mut events = before.clone();
        assert_eq!(path.frame(&mut events, 1, vec![], 1.0, pen), 0);
        assert_eq!(events, before);
    }
}
