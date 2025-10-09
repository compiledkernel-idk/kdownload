use std::io::{IsTerminal, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};

use crate::download::ProgressMode;
use crate::scheduler::Scheduler;
use crate::util::format_bytes;

const PROGRESS_TICK: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy)]
pub enum ProgressFinish {
    Success,
    Failure,
}

pub struct ProgressReporter {
    stop_tx: Option<oneshot::Sender<ProgressFinish>>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressReporter {
    pub fn spawn(
        mode: ProgressMode,
        total_bytes: Option<u64>,
        initial_downloaded: u64,
        progress: Arc<AtomicU64>,
        scheduler: Option<Arc<Scheduler>>,
    ) -> Option<Self> {
        match mode {
            ProgressMode::Quiet => None,
            ProgressMode::Text => Some(Self::spawn_text(
                total_bytes,
                initial_downloaded,
                progress,
                scheduler,
            )),
            ProgressMode::Json => Some(Self::spawn_json(
                total_bytes,
                initial_downloaded,
                progress,
                scheduler,
            )),
        }
    }

    pub async fn finish(mut self, finish: ProgressFinish) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(finish);
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }

    fn spawn_text(
        total_bytes: Option<u64>,
        initial_downloaded: u64,
        progress: Arc<AtomicU64>,
        scheduler: Option<Arc<Scheduler>>,
    ) -> Self {
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let mut ticker = interval(PROGRESS_TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut renderer = TextRenderer::new();
            let start = Instant::now();

            let initial_snapshot = build_snapshot(
                total_bytes,
                initial_downloaded,
                start,
                &progress,
                scheduler.as_ref(),
            )
            .await;
            renderer.render(&initial_snapshot, None);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let snapshot = build_snapshot(
                            total_bytes,
                            initial_downloaded,
                            start,
                            &progress,
                            scheduler.as_ref()
                        ).await;
                        renderer.render(&snapshot, None);
                    }
                    result = &mut stop_rx => {
                        let finish = result.unwrap_or(ProgressFinish::Failure);
                        let snapshot = build_snapshot(
                            total_bytes,
                            initial_downloaded,
                            start,
                            &progress,
                            scheduler.as_ref()
                        ).await;
                        renderer.render(&snapshot, Some(finish));
                        break;
                    }
                }
            }
        });

        Self {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
        }
    }

    fn spawn_json(
        total_bytes: Option<u64>,
        initial_downloaded: u64,
        progress: Arc<AtomicU64>,
        scheduler: Option<Arc<Scheduler>>,
    ) -> Self {
        let (stop_tx, mut stop_rx) = oneshot::channel();
        let handle = tokio::spawn(async move {
            let mut ticker = interval(PROGRESS_TICK);
            ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let mut renderer = JsonRenderer::new();
            let start = Instant::now();

            let initial_snapshot = build_snapshot(
                total_bytes,
                initial_downloaded,
                start,
                &progress,
                scheduler.as_ref(),
            )
            .await;
            renderer.render(&initial_snapshot, JsonRenderKind::Progress);

            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        let snapshot = build_snapshot(
                            total_bytes,
                            initial_downloaded,
                            start,
                            &progress,
                            scheduler.as_ref()
                        ).await;
                        renderer.render(&snapshot, JsonRenderKind::Progress);
                    }
                    result = &mut stop_rx => {
                        let finish = result.unwrap_or(ProgressFinish::Failure);
                        let snapshot = build_snapshot(
                            total_bytes,
                            initial_downloaded,
                            start,
                            &progress,
                            scheduler.as_ref()
                        ).await;
                        renderer.render(&snapshot, JsonRenderKind::Finish(finish));
                        break;
                    }
                }
            }
        });

        Self {
            stop_tx: Some(stop_tx),
            handle: Some(handle),
        }
    }
}

impl Drop for ProgressReporter {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            handle.abort();
        }
    }
}

struct ProgressSnapshot {
    downloaded: u64,
    total: Option<u64>,
    initial: u64,
    elapsed: Duration,
    segments_active: Option<usize>,
    segments_pending: Option<usize>,
    target_parallelism: Option<usize>,
}

impl ProgressSnapshot {
    fn percent(&self) -> Option<f64> {
        let total = self.total?;
        if total == 0 {
            return Some(100.0);
        }
        Some((self.downloaded as f64 / total as f64 * 100.0).min(100.0))
    }

    fn throughput(&self) -> f64 {
        let elapsed = self.elapsed.as_secs_f64();
        if elapsed <= f64::EPSILON {
            return 0.0;
        }
        (self.downloaded.saturating_sub(self.initial) as f64) / elapsed
    }
}

async fn build_snapshot(
    total: Option<u64>,
    initial: u64,
    start: Instant,
    progress: &Arc<AtomicU64>,
    scheduler: Option<&Arc<Scheduler>>,
) -> ProgressSnapshot {
    let downloaded = progress.load(Ordering::Relaxed);
    let scheduler_snapshot = match scheduler {
        Some(s) => Some(s.snapshot().await),
        None => None,
    };

    ProgressSnapshot {
        downloaded,
        total,
        initial,
        elapsed: start.elapsed(),
        segments_active: scheduler_snapshot.as_ref().map(|s| s.active),
        segments_pending: scheduler_snapshot.as_ref().map(|s| s.pending),
        target_parallelism: scheduler_snapshot.as_ref().map(|s| s.target_parallelism),
    }
}

struct TextRenderer {
    is_tty: bool,
    last_line_len: usize,
    last_log: Option<Instant>,
    last_line: Option<String>,
}

impl TextRenderer {
    fn new() -> Self {
        let is_tty = std::io::stderr().is_terminal();
        Self {
            is_tty,
            last_line_len: 0,
            last_log: None,
            last_line: None,
        }
    }

    fn render(&mut self, snapshot: &ProgressSnapshot, finish: Option<ProgressFinish>) {
        let mut parts = Vec::new();
        if let Some(total) = snapshot.total {
            parts.push(format!(
                "{} / {}",
                format_bytes(snapshot.downloaded),
                format_bytes(total)
            ));
        } else {
            parts.push(format!("{} downloaded", format_bytes(snapshot.downloaded)));
        }

        if let Some(percent) = snapshot.percent() {
            parts.push(format!("{percent:5.1}%"));
        }

        let throughput = snapshot.throughput();
        if throughput > 0.0 {
            let rate = format_bytes(throughput.round() as u64);
            parts.push(format!("{rate}/s"));
        }

        if let Some(active) = snapshot.segments_active {
            parts.push(format!("active:{}", active));
        }
        if let Some(pending) = snapshot.segments_pending {
            parts.push(format!("pending:{}", pending));
        }
        if let Some(target) = snapshot.target_parallelism {
            parts.push(format!("target:{}", target));
        }

        let line = parts.join(" • ");
        if self.is_tty {
            let mut to_print = line.clone();
            if self.last_line_len > line.len() {
                let padding = " ".repeat(self.last_line_len - line.len());
                to_print.push_str(&padding);
            }
            eprint!("\r{}", to_print);
            let _ = std::io::stderr().flush();
            self.last_line_len = line.len();
            if finish.is_some() {
                eprintln!();
            }
        } else {
            let now = Instant::now();
            let is_new_line = self.last_line.as_ref().map_or(true, |prev| prev != &line);
            let should_emit = finish.is_some()
                || is_new_line
                || self.last_log.map_or(true, |prev| {
                    now.duration_since(prev) >= Duration::from_secs(1)
                });
            if should_emit {
                println!("{}", line);
                let _ = std::io::stdout().flush();
                self.last_log = Some(now);
                self.last_line = Some(line);
            }
        }
    }
}

struct JsonRenderer;

impl JsonRenderer {
    fn new() -> Self {
        Self
    }

    fn render(&mut self, snapshot: &ProgressSnapshot, kind: JsonRenderKind) {
        let event = match kind {
            JsonRenderKind::Progress => JsonProgressEvent::progress(snapshot),
            JsonRenderKind::Finish(outcome) => JsonProgressEvent::finish(snapshot, outcome),
        };
        if let Ok(serialized) = serde_json::to_string(&event) {
            println!("{}", serialized);
            let _ = std::io::stdout().flush();
        }
    }
}

enum JsonRenderKind {
    Progress,
    Finish(ProgressFinish),
}

#[derive(Serialize)]
struct JsonProgressEvent {
    event: &'static str,
    timestamp_ms: u128,
    elapsed_ms: u128,
    bytes_downloaded: u64,
    total_bytes: Option<u64>,
    fraction: Option<f64>,
    bytes_per_second: f64,
    active_segments: Option<usize>,
    pending_segments: Option<usize>,
    target_parallelism: Option<usize>,
}

impl JsonProgressEvent {
    fn progress(snapshot: &ProgressSnapshot) -> Self {
        Self::from_snapshot("progress", snapshot)
    }

    fn finish(snapshot: &ProgressSnapshot, finish: ProgressFinish) -> Self {
        let event = match finish {
            ProgressFinish::Success => "complete",
            ProgressFinish::Failure => "failed",
        };
        Self::from_snapshot(event, snapshot)
    }

    fn from_snapshot(event: &'static str, snapshot: &ProgressSnapshot) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let elapsed_ms = snapshot.elapsed.as_millis();
        let fraction = snapshot.percent().map(|p| p / 100.0);
        let bytes_per_second = snapshot.throughput();

        JsonProgressEvent {
            event,
            timestamp_ms: now,
            elapsed_ms,
            bytes_downloaded: snapshot.downloaded,
            total_bytes: snapshot.total,
            fraction,
            bytes_per_second,
            active_segments: snapshot.segments_active,
            pending_segments: snapshot.segments_pending,
            target_parallelism: snapshot.target_parallelism,
        }
    }
}
