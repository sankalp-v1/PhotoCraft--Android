//! The app's own frame pacing, for frames that don't wait for the display.
//!
//! Presented with vsync, every frame joined a queue behind the display: a pen reports 240
//! positions a second, each asked for a frame, and egui draws one more after any frame with
//! input, so the app always had more frames ready than the screen could show. The queue stayed
//! full, and what was on screen was two or three refreshes old (Android port #11: 34 ms from the
//! start of a frame to the screen at 90 Hz, of which 23 ms waiting in the queue).
//!
//! With low latency presentation (Touch Input Settings, on by default) frames don't queue: the
//! screen shows the newest one at each refresh. Nothing then holds the app back, so it paces
//! itself: while the pen, a finger or a mouse is at work a frame may start twice per refresh
//! (between [`MIN_ACTIVE_FPS`] and [`ACTIVE_FPS`] times a second); otherwise (an animation, a spinner) at the display's own rate, which is all the
//! screen can show. A frame waiting for the display's turn starts at once when input arrives.

use std::time::Duration;

/// The most frames a second while input is arriving: a pen's report rate, so each sample can
/// have its own frame where the device is quick enough.
pub const ACTIVE_FPS: f32 = 240.0;
/// The fewest frames a second while input is arriving. Between the two, twice the display's
/// rate: a frame starts at most half a refresh after the pen moved, and a 60 Hz tablet doesn't
/// draw four frames for every one it shows (heat, and a GPU busy with frames nobody sees, would
/// slow the ones that are).
pub const MIN_ACTIVE_FPS: f32 = 120.0;
/// How long after an input event the app still counts as being worked with.
pub const ACTIVE_FOR: Duration = Duration::from_millis(150);
/// The display's refresh rate where the Activity hasn't said (or said nonsense).
pub const DEFAULT_HZ: f32 = 60.0;

/// The shortest time between two frame starts: `active` while input is arriving, else the
/// display's refresh period (a little under it, so a steady animation never slips a refresh).
pub fn interval(refresh_hz: Option<f32>, active: bool) -> Duration {
    let hz = refresh_hz.filter(|hz| hz.is_finite() && (24.0..=480.0).contains(hz)).unwrap_or(DEFAULT_HZ);
    if active {
        return Duration::from_secs_f32(1.0 / (2.0 * hz).clamp(MIN_ACTIVE_FPS, ACTIVE_FPS));
    }
    Duration::from_secs_f32(0.98 / hz)
}

/// How long a frame should still wait before it starts, `since_last` after the last one did.
pub fn wait(since_last: Duration, refresh_hz: Option<f32>, active: bool) -> Duration {
    interval(refresh_hz, active).saturating_sub(since_last)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(d: Duration) -> f64 {
        d.as_secs_f64() * 1000.0
    }

    #[test]
    fn input_gets_a_frame_within_half_a_refresh() {
        // A frame with input waits at most half a refresh after the last start (1/120 s at most,
        // 1/240 s at least), whatever the display.
        for (hz, most) in [(None, 8.4), (Some(60.0), 8.4), (Some(90.0), 5.6), (Some(120.0), 4.2), (Some(144.0), 4.2), (Some(240.0), 4.2)] {
            let w = ms(wait(Duration::ZERO, hz, true));
            assert!(w < most && w > 4.1, "{hz:?}: {w}");
            assert_eq!(wait(Duration::from_millis(9), hz, true), Duration::ZERO, "a frame slower than that never waits");
        }
    }

    #[test]
    fn an_animation_runs_at_the_displays_rate() {
        assert!((ms(interval(Some(60.0), false)) - 16.33).abs() < 0.05);
        assert!((ms(interval(Some(90.0), false)) - 10.89).abs() < 0.05);
        assert!((ms(interval(Some(120.0), false)) - 8.17).abs() < 0.05);
        // 3 ms into a 120 Hz refresh: the rest of it.
        assert!((ms(wait(Duration::from_millis(3), Some(120.0), false)) - 5.17).abs() < 0.05);
        assert_eq!(wait(Duration::from_millis(40), Some(120.0), false), Duration::ZERO);
    }

    #[test]
    fn a_missing_or_absurd_refresh_rate_counts_as_60_hz() {
        let sixty = interval(Some(60.0), false);
        for hz in [None, Some(0.0), Some(-90.0), Some(f32::NAN), Some(f32::INFINITY), Some(5.0), Some(100_000.0)] {
            assert_eq!(interval(hz, false), sixty, "{hz:?}");
        }
    }

    /// A second of a stroke on a simulated display. The app's frames take `work_ms`, the pen
    /// reports every 1/240 s and always has something new, so the next frame starts as soon as it
    /// may. `queue: Some(n)` is the old presentation: finished frames wait in line, one shown per
    /// refresh, and the app is held up while `n` are waiting. `None` is the new one: each refresh
    /// shows the newest finished frame, and the app paces itself ([`interval`]). Returns the mean
    /// age (ms), at a refresh, of the newest pen sample in the frame it shows, and the frames drawn.
    fn simulate(hz: f32, work_ms: f64, queue: Option<usize>) -> (f64, usize) {
        let period = 1000.0 / hz as f64;
        let sample = 1000.0 / 240.0;
        // When each frame was finished and handed over, and the newest sample it drew.
        let mut frames: Vec<(f64, f64)> = Vec::new();
        // With a queue: the refresh each frame is shown at.
        let mut shown: Vec<f64> = Vec::new();
        let (mut t, mut last_start) = (0.0f64, f64::NEG_INFINITY);
        while t < 1000.0 {
            let start = if queue.is_none() { t.max(last_start + ms(interval(Some(hz), true))) } else { t };
            let newest = (start / sample).floor() * sample;
            let mut done = start + work_ms;
            if let Some(depth) = queue {
                // No room until the frame `depth` back has left the queue for the screen.
                if shown.len() >= depth {
                    done = done.max(shown[shown.len() - depth]);
                }
                let after_last = shown.last().map_or(0.0, |s| s + period);
                shown.push(((done / period).ceil() * period).max(after_last));
            }
            frames.push((done, newest));
            last_start = start;
            t = done;
        }
        let mut ages = Vec::new();
        let mut refresh = period * 10.0;
        while refresh < 1000.0 {
            let on_screen = match queue {
                Some(_) => frames.iter().zip(&shown).rfind(|(_, at)| **at <= refresh + 1e-6).map(|(f, _)| f.1),
                None => frames.iter().rfind(|f| f.0 <= refresh + 1e-6).map(|f| f.1),
            };
            if let Some(newest) = on_screen {
                ages.push(refresh - newest);
            }
            refresh += period;
        }
        (ages.iter().sum::<f64>() / ages.len().max(1) as f64, frames.len())
    }

    #[test]
    fn frames_that_dont_queue_put_newer_ink_on_screen() {
        // Measured against the old presentation (a queue of two frames behind the display) on
        // displays from 60 to 144 Hz and devices from quick (2 ms a frame) to slow (14 ms).
        for hz in [60.0f32, 90.0, 120.0, 144.0] {
            for work in [2.0, 4.0, 6.0, 8.0, 10.0, 14.0] {
                let (queued, _) = simulate(hz, work, Some(2));
                let (paced, frames) = simulate(hz, work, None);
                assert!(paced <= queued + 0.01, "{hz} Hz, {work} ms a frame: {paced:.1} ms paced, {queued:.1} ms queued");
                assert!(frames <= 241, "{hz} Hz, {work} ms: {frames} frames in a second");
                assert!(frames as f32 <= 2.0 * hz.max(60.0) + 1.0, "{hz} Hz, {work} ms: {frames} frames for {hz} refreshes");
                if work < 1000.0 / hz as f64 {
                    // A device that keeps up with its display: at least half a refresh sooner.
                    assert!(queued - paced >= 500.0 / hz as f64, "{hz} Hz, {work} ms a frame: {paced:.1} ms paced, {queued:.1} ms queued");
                }
            }
        }
    }
}
