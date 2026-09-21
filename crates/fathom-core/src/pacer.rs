//! How many simulation steps fit in a frame.
//!
//! The interface wants a steady frame rate; the simulation wants every step it can
//! get. The pacer sits between them: each frame the app is told how many steps to run,
//! and afterwards the pacer is told how long the frame's work took. Under budget it asks
//! for more steps next time, over budget it cuts back in proportion, and within a dead
//! band around the target it holds still, so the count settles rather than hunting.
//!
//! Work time is smoothed with an exponential moving average, so one slow frame — a
//! shader compile, a readback — does not throw the count around. Nothing is regulated
//! for the first few frames: the average has to mean something first.

/// Step budgeting for one frame at a time.
#[derive(Clone, Debug)]
pub struct Pacer {
    steps: u32,
    work_ms: f32,
    frames: u32,
    /// Work per frame the pacer aims for, in milliseconds.
    pub target_ms: f32,
    /// Half-width of the band around the target inside which nothing changes.
    pub dead_ms: f32,
    /// The most steps a frame is ever asked for.
    pub max_steps: u32,
    /// Frames observed before regulation starts.
    pub warmup: u32,
}

impl Default for Pacer {
    /// Aims a little under 60 fps worth of work — 12 ms of the frame's 16.7 — so the
    /// panel and the present have room of their own.
    fn default() -> Self {
        Self { steps: 1, work_ms: 0.0, frames: 0, target_ms: 12.0, dead_ms: 1.5, max_steps: 4096, warmup: 20 }
    }
}

impl Pacer {
    /// The steps to run this frame.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Smoothed work time per frame, in milliseconds.
    pub fn work_ms(&self) -> f32 {
        self.work_ms
    }

    /// Start over at one step: the scene changed size, or the app asked.
    pub fn reset(&mut self) {
        self.steps = 1;
        self.work_ms = 0.0;
        self.frames = 0;
    }

    /// Report how long this frame's work took and settle on next frame's count.
    pub fn observe(&mut self, work_ms: f32) {
        if !work_ms.is_finite() || work_ms < 0.0 {
            return;
        }
        // A stall is not a slow frame: it counts as a bad one, not as a catastrophe.
        let work_ms = work_ms.min(4.0 * self.target_ms);
        self.work_ms = if self.frames == 0 { work_ms } else { self.work_ms + (work_ms - self.work_ms) * 0.15 };
        self.frames += 1;
        if self.frames <= self.warmup {
            return;
        }
        let work = self.work_ms.max(0.05);
        if work < self.target_ms - self.dead_ms {
            // Under: ramp up by five percent, and by at least one.
            self.steps = (self.steps + (self.steps / 20).max(1)).min(self.max_steps);
        } else if work > self.target_ms + self.dead_ms {
            // Over: pull back in proportion, but never to nothing.
            self.steps = ((self.steps as f32 * self.target_ms / work) as u32).max(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warmed(pacer: &mut Pacer, work_ms: f32) {
        for _ in 0..pacer.warmup {
            pacer.observe(work_ms);
        }
    }

    #[test]
    fn starts_at_one_step_and_holds_through_warmup() {
        let mut p = Pacer::default();
        assert_eq!(p.steps(), 1);
        warmed(&mut p, 1.0);
        assert_eq!(p.steps(), 1);
    }

    #[test]
    fn ramps_up_while_under_budget_and_stops_at_the_cap() {
        let mut p = Pacer { max_steps: 100, ..Default::default() };
        warmed(&mut p, 1.0);
        let before = p.steps();
        p.observe(1.0);
        assert!(p.steps() > before);
        for _ in 0..2000 {
            p.observe(1.0);
        }
        assert_eq!(p.steps(), 100);
    }

    #[test]
    fn cuts_back_in_proportion_when_over_budget() {
        let mut p = Pacer::default();
        warmed(&mut p, 1.0);
        for _ in 0..200 {
            p.observe(1.0);
        }
        let many = p.steps();
        assert!(many > 50, "{many}");
        // The frame suddenly costs four times the target: the count drops toward a
        // quarter, never below one.
        let slow = 4.0 * p.target_ms;
        for _ in 0..40 {
            p.observe(slow);
        }
        assert!(p.steps() < many / 3, "{} of {many}", p.steps());
        for _ in 0..200 {
            p.observe(1000.0);
        }
        assert_eq!(p.steps(), 1);
    }

    #[test]
    fn holds_still_inside_the_dead_band() {
        let mut p = Pacer::default();
        let target = p.target_ms;
        warmed(&mut p, target);
        let held = p.steps();
        let (high, low) = (p.target_ms + 0.5 * p.dead_ms, p.target_ms - 0.5 * p.dead_ms);
        for _ in 0..50 {
            p.observe(high);
            p.observe(low);
        }
        assert_eq!(p.steps(), held);
    }

    #[test]
    fn one_slow_frame_does_not_move_the_count() {
        let mut p = Pacer::default();
        warmed(&mut p, 1.0);
        for _ in 0..100 {
            p.observe(1.0);
        }
        let before = p.steps();
        p.observe(500.0);
        // The stall is clipped before it is averaged: the count barely moves.
        assert!(p.steps() >= before, "{} of {before}", p.steps());
        assert!(!p.work_ms().is_nan());
        p.observe(f32::NAN);
        assert!(!p.work_ms().is_nan());
    }
}
