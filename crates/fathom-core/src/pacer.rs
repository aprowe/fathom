//! How many simulation steps fit in a frame.
//!
//! The interface wants a steady frame rate; the simulation wants every step it can
//! get. The pacer sits between them: each frame the app is told how many steps to run,
//! and when a measurement of some earlier batch comes back — how many steps it ran and
//! how long they took on the GPU — the pacer is told. Measurements arrive late (a
//! readback lands a few frames after its batch) and not every frame, so the pacer does
//! not regulate on "was the last frame over budget"; it keeps a smoothed **cost per
//! step**, which is the same whichever batch it was measured on, and sets the count to
//! what the budget buys at that cost.
//!
//! The count changes only when a fresh measurement lands. It is cut when the current
//! count no longer fits the budget, and raised only when the next count would fit with
//! some slack to spare — a hysteresis, so a cost drifting across a quantisation edge
//! does not flip the count (and with it the simulation's speed) back and forth. A rise
//! is never more than half again at once; a cut is immediate, since an over-budget
//! frame is a dropped frame.

/// Step budgeting for one frame at a time.
#[derive(Clone, Debug)]
pub struct Pacer {
    steps: u32,
    cost_ms: f32,
    samples: u32,
    /// Consecutive samples past the stall clip: two in a row are a real change of
    /// cost, not a stall.
    high_streak: u32,
    /// Work per frame the pacer aims for, in milliseconds.
    pub target_ms: f32,
    /// The count is raised only when the next count fits the budget with this
    /// fraction to spare.
    pub slack: f32,
    /// The most steps a frame is ever asked for.
    pub max_steps: u32,
    /// Measurements observed before regulation starts: the cost has to mean something.
    pub warmup: u32,
}

impl Default for Pacer {
    /// Aims at 10 ms of steps in a 16.7 ms frame: the draw, the panel and the present
    /// need the rest, and a frame that overruns is a frame the display skips.
    fn default() -> Self {
        Self { steps: 1, cost_ms: 0.0, samples: 0, high_streak: 0, target_ms: 10.0, slack: 0.15, max_steps: 4096, warmup: 3 }
    }
}

impl Pacer {
    /// The steps to run this frame.
    pub fn steps(&self) -> u32 {
        self.steps
    }

    /// Smoothed cost of one step, in milliseconds.
    pub fn cost_ms(&self) -> f32 {
        self.cost_ms
    }

    /// What the current count is expected to take, in milliseconds.
    pub fn work_ms(&self) -> f32 {
        self.steps as f32 * self.cost_ms
    }

    /// Start over at one step: the scene changed size, or the app asked.
    pub fn reset(&mut self) {
        self.steps = 1;
        self.cost_ms = 0.0;
        self.samples = 0;
        self.high_streak = 0;
    }

    /// A measurement of some batch: it ran `steps` steps in `work_ms`. Call once per
    /// measurement, never with a repeated one: each call is treated as news.
    pub fn observe(&mut self, steps: u32, work_ms: f32) {
        if steps == 0 || !work_ms.is_finite() || work_ms <= 0.0 {
            return;
        }
        // A stall is not a slow step: one sample past a few times the cost known so
        // far is clipped, so it counts as a bad sample, not a catastrophe. Two in a
        // row are a real change of cost — a bigger cutoff, a soup that clustered —
        // and are believed outright, so a step that got sixty times dearer is cut in
        // one reading, not five.
        let raw = work_ms / steps as f32;
        let clip = if self.samples == 0 { 4.0 * self.target_ms } else { 4.0 * self.cost_ms };
        let high = raw > clip;
        self.high_streak = if high { self.high_streak + 1 } else { 0 };
        let cost = if high && self.high_streak < 2 { clip } else { raw };
        self.cost_ms = if self.samples == 0 || self.high_streak >= 2 { cost } else { self.cost_ms + (cost - self.cost_ms) * 0.3 };
        self.samples += 1;
        if self.samples < self.warmup {
            return;
        }
        let cost = self.cost_ms.max(1e-3);
        let wanted = ((self.target_ms / cost) as u32).clamp(1, self.max_steps);
        if self.steps as f32 * cost > self.target_ms {
            self.steps = wanted;
        } else if (self.steps + 1) as f32 * cost * (1.0 + self.slack) <= self.target_ms {
            self.steps = wanted.min(self.steps + (self.steps / 2).max(1)).max(self.steps);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A frame loop with a GPU whose readback lands `latency` frames after the batch:
    /// each frame runs the pacer's count, and the measurement of the batch from
    /// `latency` frames ago arrives, if that frame's batch exists. `cost` is the true
    /// milliseconds per step. Returns the count run each frame.
    fn run(pacer: &mut Pacer, frames: usize, latency: usize, cost: impl Fn(usize) -> f32) -> Vec<u32> {
        let mut history: Vec<(u32, f32)> = Vec::new();
        let mut counts = Vec::new();
        for f in 0..frames {
            let steps = pacer.steps();
            counts.push(steps);
            history.push((steps, steps as f32 * cost(f)));
            if f >= latency {
                let (s, ms) = history[f - latency];
                pacer.observe(s, ms);
            }
        }
        counts
    }

    #[test]
    fn starts_at_one_step_and_holds_through_warmup() {
        let mut p = Pacer::default();
        assert_eq!(p.steps(), 1);
        for _ in 0..p.warmup - 1 {
            p.observe(1, 1.0);
        }
        assert_eq!(p.steps(), 1);
    }

    /// The count settles at what the budget buys and then holds still, even though
    /// every measurement is three frames old: a late sample still says what a step
    /// costs.
    #[test]
    fn settles_without_oscillating_under_a_late_readback() {
        let mut p = Pacer::default();
        let cost = 0.4;
        let counts = run(&mut p, 200, 3, |_| cost);
        let tail = &counts[100..];
        let (lo, hi) = (*tail.iter().min().unwrap(), *tail.iter().max().unwrap());
        let expect = (p.target_ms / cost) as u32;
        assert!(hi.abs_diff(expect) <= 3, "settled at {lo}..{hi}, expected about {expect}");
        assert!(hi - lo <= 2, "the count saws between {lo} and {hi}");
        assert!(tail.iter().all(|&s| s as f32 * cost <= p.target_ms * 1.15), "over budget in the tail");
    }

    /// One over-budget measurement cuts once. Nothing repeats it: the count does not
    /// collapse to one because the same late sample kept arriving.
    #[test]
    fn a_cut_is_proportional_and_happens_once() {
        let mut p = Pacer::default();
        run(&mut p, 100, 2, |_| 0.4);
        let settled = p.steps();
        assert!(settled > 10, "{settled}");
        // Steps suddenly cost twice as much: the count comes down toward half, and
        // stays there.
        p.observe(settled, settled as f32 * 0.8);
        let after = p.steps();
        assert!(after >= settled / 3 && after < settled, "{after} of {settled}");
        // Cost stays doubled: a few more samples and it holds near half, not at one.
        for _ in 0..10 {
            let s = p.steps();
            p.observe(s, s as f32 * 0.8);
        }
        assert!(p.steps() >= settled / 3, "collapsed to {}", p.steps());
        assert!(p.steps() <= settled * 2 / 3 + 1, "{}", p.steps());
    }

    /// A cost that drifts across a quantisation edge — two steps fit, then just do
    /// not, then do — must not flip the count back and forth: that is a speed that
    /// halves and doubles every few frames.
    #[test]
    fn no_flip_flop_at_a_quantisation_edge() {
        let mut p = Pacer::default();
        let counts = run(&mut p, 400, 3, |f| if (f / 20) % 2 == 0 { 4.85 } else { 5.05 });
        let tail = &counts[100..];
        let changes = tail.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(changes <= 1, "the count changed {changes} times in the tail: {:?}", &tail[..40]);
    }

    /// A step that suddenly costs sixty times more — the count went from four
    /// thousand particles to sixty-five thousand — is believed by the second reading
    /// and the count drops to what fits at once, not by halves over many readings.
    #[test]
    fn a_sustained_jump_in_cost_is_believed_by_the_second_reading() {
        let mut p = Pacer::default();
        run(&mut p, 100, 2, |_| 0.34);
        let before = p.steps();
        assert!(before > 20, "{before}");
        p.observe(before, before as f32 * 20.0);
        p.observe(before, before as f32 * 20.0);
        assert_eq!(p.steps(), 1, "after two readings at 20 ms a step: {}", p.steps());
        assert!((p.cost_ms() - 20.0).abs() < 1.0, "{}", p.cost_ms());
        // A reset forgets the cost: the first reading afterwards is taken whole.
        p.reset();
        assert_eq!(p.steps(), 1);
        p.observe(1, 20.0);
        assert!((p.cost_ms() - 20.0).abs() < 1e-3);
    }

    #[test]
    fn ramps_up_gently_and_stops_at_the_cap() {
        let mut p = Pacer { max_steps: 100, ..Default::default() };
        let counts = run(&mut p, 60, 1, |_| 0.001);
        // Never more than half again per fresh sample.
        for w in counts.windows(2) {
            assert!(w[1] <= w[0] + (w[0] / 2).max(1), "{:?}", w);
        }
        assert_eq!(p.steps(), 100);
    }

    #[test]
    fn a_stall_and_a_bad_sample_do_not_move_the_count_much() {
        let mut p = Pacer::default();
        run(&mut p, 100, 2, |_| 0.4);
        let before = p.steps();
        p.observe(before, 5000.0);
        assert!(p.steps() >= before / 8, "{} of {before}", p.steps());
        p.observe(before, f32::NAN);
        p.observe(0, 1.0);
        p.observe(before, -1.0);
        assert!(!p.cost_ms().is_nan());
        assert_eq!(p.work_ms(), p.steps() as f32 * p.cost_ms());
    }
}
