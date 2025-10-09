use std::collections::VecDeque;
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub struct SegmentTask {
    pub id: usize,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl SegmentTask {
    pub fn remaining_range(&self) -> Option<(u64, u64)> {
        let total = self.end.saturating_sub(self.start) + 1;
        if self.downloaded >= total {
            None
        } else {
            let begin = self.start + self.downloaded;
            Some((begin, self.end))
        }
    }

    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }
}

#[derive(Debug, Clone)]
pub struct SegmentStats {
    pub id: usize,
    pub bytes: u64,
    pub duration: Duration,
}

impl SegmentStats {
    pub fn throughput(&self) -> f64 {
        if self.duration.is_zero() {
            return self.bytes as f64;
        }
        self.bytes as f64 / self.duration.as_secs_f64()
    }
}

struct SchedulerState {
    pending: VecDeque<SegmentTask>,
    active: usize,
    target_parallelism: usize,
    recent_speeds: VecDeque<f64>,
    last_adjustment: Instant,
}

pub struct Scheduler {
    state: Mutex<SchedulerState>,
    max_parallelism: usize,
    throughput_window: usize,
    scale_up_threshold: f64,
    scale_down_threshold: f64,
    adjustment_interval: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct SchedulerSnapshot {
    pub pending: usize,
    pub active: usize,
    pub target_parallelism: usize,
}

impl Scheduler {
    pub fn new(
        initial_segments: Vec<SegmentTask>,
        initial_parallelism: usize,
        max_parallelism: usize,
    ) -> Self {
        Self {
            state: Mutex::new(SchedulerState {
                pending: initial_segments.into_iter().collect::<VecDeque<_>>(),
                active: 0,
                target_parallelism: initial_parallelism.clamp(1, max_parallelism.max(1)),
                recent_speeds: VecDeque::new(),
                last_adjustment: Instant::now(),
            }),
            max_parallelism: max_parallelism.max(1),
            throughput_window: 24,
            scale_up_threshold: 8_000_000.0, // ~8 MiB/s per connection
            scale_down_threshold: 200_000.0, // ~200 KiB/s per connection
            adjustment_interval: Duration::from_secs(2),
        }
    }

    pub async fn next_segment(&self) -> Option<SegmentTask> {
        let mut state = self.state.lock().await;
        if state.active >= state.target_parallelism {
            return None;
        }
        if let Some(segment) = state.pending.pop_front() {
            state.active += 1;
            Some(segment)
        } else {
            None
        }
    }

    pub async fn on_segment_complete(&self, stats: SegmentStats) {
        let mut state = self.state.lock().await;
        if state.active > 0 {
            state.active -= 1;
        }
        state.recent_speeds.push_back(stats.throughput());
        if state.recent_speeds.len() > self.throughput_window {
            state.recent_speeds.pop_front();
        }

        let now = Instant::now();
        if now.duration_since(state.last_adjustment) < self.adjustment_interval {
            return;
        }
        state.last_adjustment = now;

        if state.recent_speeds.is_empty() {
            return;
        }

        let total_speed: f64 = state.recent_speeds.iter().copied().sum();
        let avg_speed = total_speed / state.recent_speeds.len() as f64;
        let active = state.target_parallelism.max(1) as f64;
        let per_conn = avg_speed / active;

        if per_conn > self.scale_up_threshold && state.target_parallelism < self.max_parallelism {
            state.target_parallelism += 1;
        } else if per_conn < self.scale_down_threshold && state.target_parallelism > 1 {
            state.target_parallelism -= 1;
        }
    }

    pub async fn has_remaining(&self) -> bool {
        let state = self.state.lock().await;
        !state.pending.is_empty() || state.active > 0
    }

    pub async fn snapshot(&self) -> SchedulerSnapshot {
        let state = self.state.lock().await;
        SchedulerSnapshot {
            pending: state.pending.len(),
            active: state.active,
            target_parallelism: state.target_parallelism,
        }
    }
}
