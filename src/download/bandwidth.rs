use std::time::Instant;

use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};

pub struct BandwidthLimiter {
    limit_per_sec: f64,
    state: Mutex<LimiterState>,
}

struct LimiterState {
    tokens: f64,
    last: Instant,
}

impl BandwidthLimiter {
    pub fn new(limit_per_sec: u64) -> Self {
        Self {
            limit_per_sec: limit_per_sec as f64,
            state: Mutex::new(LimiterState {
                tokens: limit_per_sec as f64,
                last: Instant::now(),
            }),
        }
    }

    pub async fn consume(&self, amount: usize) {
        let amount = amount as f64;
        loop {
            let mut state = self.state.lock().await;
            let elapsed = state.last.elapsed().as_secs_f64();
            if elapsed > 0.0 {
                state.tokens =
                    (state.tokens + elapsed * self.limit_per_sec).min(self.limit_per_sec * 2.0);
                state.last = Instant::now();
            }

            if state.tokens >= amount {
                state.tokens -= amount;
                return;
            }

            let deficit = amount - state.tokens;
            let wait_secs = (deficit / self.limit_per_sec).max(0.01);
            state.last = Instant::now();
            drop(state);
            sleep(Duration::from_secs_f64(wait_secs)).await;
        }
    }
}
