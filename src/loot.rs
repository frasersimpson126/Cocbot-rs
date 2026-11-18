use std::time::{Duration, Instant};

/// Resource totals read from the Home Village HUD.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub gold: u64,
    pub elixir: u64,
    pub dark_elixir: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LootDelta {
    pub gold: u64,
    pub elixir: u64,
    pub dark_elixir: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct LootSummary {
    pub total: LootDelta,
    pub per_hour: LootDelta,
    pub elapsed: Duration,
}

#[derive(Debug)]
pub struct LootTracker {
    total: LootDelta,
    previous: Option<ResourceSnapshot>,
    started: Instant,
    // Deliberately conservative guards against OCR glitches between adjacent reads.
    max_main_delta: u64,
    max_dark_delta: u64,
}

impl Default for LootTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl LootTracker {
    pub fn new() -> Self {
        Self {
            total: LootDelta::default(),
            previous: None,
            started: Instant::now(),
            max_main_delta: 3_000_000,
            max_dark_delta: 50_000,
        }
    }

    pub fn reset(&mut self) {
        self.total = LootDelta::default();
        self.previous = None;
        self.started = Instant::now();
    }

    /// Establishes the initial baseline. Subsequent calls add only plausible, non-negative
    /// gains. A rejected snapshot still becomes the next baseline to avoid repeated errors.
    pub fn observe(&mut self, snapshot: ResourceSnapshot) -> Option<LootDelta> {
        let Some(previous) = self.previous.replace(snapshot) else {
            return None;
        };

        let delta = LootDelta {
            gold: snapshot.gold.checked_sub(previous.gold)?,
            elixir: snapshot.elixir.checked_sub(previous.elixir)?,
            dark_elixir: snapshot.dark_elixir.checked_sub(previous.dark_elixir)?,
        };

        if delta.gold > self.max_main_delta
            || delta.elixir > self.max_main_delta
            || delta.dark_elixir > self.max_dark_delta
        {
            return None;
        }

        self.total.gold += delta.gold;
        self.total.elixir += delta.elixir;
        self.total.dark_elixir += delta.dark_elixir;
        Some(delta)
    }

    pub fn summary(&self) -> LootSummary {
        let elapsed = self.started.elapsed();
        let hours = elapsed.as_secs_f64() / 3600.0;
        let per_hour = if hours > 0.0 {
            LootDelta {
                gold: (self.total.gold as f64 / hours) as u64,
                elixir: (self.total.elixir as f64 / hours) as u64,
                dark_elixir: (self.total.dark_elixir as f64 / hours) as u64,
            }
        } else {
            LootDelta::default()
        };
        LootSummary {
            total: self.total,
            per_hour,
            elapsed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_observation_sets_baseline_only() {
        let mut tracker = LootTracker::new();
        assert_eq!(
            tracker.observe(ResourceSnapshot { gold: 100, elixir: 200, dark_elixir: 30 }),
            None
        );
        assert_eq!(tracker.summary().total, LootDelta::default());
    }

    #[test]
    fn plausible_positive_deltas_accumulate() {
        let mut tracker = LootTracker::new();
        tracker.observe(ResourceSnapshot { gold: 100, elixir: 200, dark_elixir: 30 });
        assert_eq!(
            tracker.observe(ResourceSnapshot { gold: 500, elixir: 900, dark_elixir: 80 }),
            Some(LootDelta { gold: 400, elixir: 700, dark_elixir: 50 })
        );
        assert_eq!(
            tracker.summary().total,
            LootDelta { gold: 400, elixir: 700, dark_elixir: 50 }
        );
    }

    #[test]
    fn resource_spending_invalidates_delta_without_counting_it() {
        let mut tracker = LootTracker::new();
        tracker.observe(ResourceSnapshot { gold: 1000, elixir: 1000, dark_elixir: 100 });
        assert_eq!(
            tracker.observe(ResourceSnapshot { gold: 900, elixir: 1200, dark_elixir: 100 }),
            None
        );
        assert_eq!(tracker.summary().total, LootDelta::default());
    }

    #[test]
    fn implausible_ocr_spike_is_rejected() {
        let mut tracker = LootTracker::new();
        tracker.observe(ResourceSnapshot { gold: 100, elixir: 200, dark_elixir: 30 });
        assert_eq!(
            tracker.observe(ResourceSnapshot { gold: 4_000_101, elixir: 200, dark_elixir: 30 }),
            None
        );
        assert_eq!(tracker.summary().total, LootDelta::default());
    }
}
