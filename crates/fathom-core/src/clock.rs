//! Frame timing.

use serde::Serialize;

/// Smoothed timing, surfaced to the interface's toolbar.
#[derive(Clone, Copy, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameStats {
    pub frame: u64,
    pub fps: f32,
    pub frame_ms: f32,
    /// Seconds since setup, excluding time spent paused.
    pub time: f32,
}

/// Turns wall-clock timestamps into a usable `dt`.
///
/// A tab left in the background, or a breakpoint, produces an enormous gap; feeding
/// that straight into an integrator explodes the simulation. `dt` is therefore clamped
/// rather than trusted.
pub struct Clock {
    last_ms: Option<f64>,
    pub frame: u64,
    pub time: f32,
    fps: f32,
    frame_ms: f32,
    max_dt: f32,
    /// Frames slower than this are treated as stalls and left out of the counters.
    max_counted_dt: f32,
}

impl Default for Clock {
    fn default() -> Self {
        Self { last_ms: None, frame: 0, time: 0.0, fps: 0.0, frame_ms: 0.0, max_dt: 1.0 / 15.0, max_counted_dt: 0.25 }
    }
}

impl Clock {
    /// Advance to `now_ms` and return the clamped delta in seconds.
    pub fn tick(&mut self, now_ms: f64) -> f32 {
        let raw = match self.last_ms {
            Some(last) => ((now_ms - last) / 1000.0) as f32,
            None => 0.0,
        };
        self.last_ms = Some(now_ms);
        self.frame += 1;

        // A stall — a backgrounded tab, a breakpoint, a scene reallocation — is not a
        // slow frame, and folding it into the counters makes them lie for seconds
        // afterwards. Such frames still advance the simulation; they just do not count.
        let blend = 0.1;
        if raw > 0.0 && raw <= self.max_counted_dt {
            self.frame_ms += (raw * 1000.0 - self.frame_ms) * blend;
            self.fps += (1.0 / raw - self.fps) * blend;
        }

        let dt = raw.clamp(0.0, self.max_dt);
        self.time += dt;
        dt
    }

    pub fn stats(&self) -> FrameStats {
        FrameStats { frame: self.frame, fps: self.fps, frame_ms: self.frame_ms, time: self.time }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_tick_has_no_elapsed_time() {
        let mut clock = Clock::default();
        assert_eq!(clock.tick(1_000.0), 0.0);
        assert_eq!(clock.frame, 1);
    }

    #[test]
    fn a_long_stall_is_clamped_instead_of_exploding_the_integrator() {
        let mut clock = Clock::default();
        clock.tick(0.0);
        let dt = clock.tick(30_000.0);
        assert!(dt <= 1.0 / 15.0 + 1e-6, "dt was {dt}");
    }

    #[test]
    fn a_stall_does_not_poison_the_frame_counters() {
        let mut clock = Clock::default();
        let mut now = 0.0;
        for _ in 0..400 {
            now += 16.6667;
            clock.tick(now);
        }
        let settled = clock.stats();

        // The tab goes to the background for two seconds, then resumes.
        now += 2_000.0;
        clock.tick(now);
        now += 16.6667;
        clock.tick(now);

        let after = clock.stats();
        assert!((after.fps - settled.fps).abs() < 1.0, "fps jumped to {}", after.fps);
        assert!(after.frame_ms < 20.0, "frame time jumped to {}", after.frame_ms);
    }

    #[test]
    fn steady_frames_converge_on_the_right_fps() {
        let mut clock = Clock::default();
        for i in 0..400 {
            clock.tick(i as f64 * 16.6667);
        }
        assert!((clock.stats().fps - 60.0).abs() < 1.0, "{}", clock.stats().fps);
    }
}
