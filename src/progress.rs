use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{interval, MissedTickBehavior};

use crate::download::ProgressMode;
use crate::scheduler::Scheduler;

const PROGRESS_TICK: Duration = Duration::from_millis(100);

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
            let mut renderer = TextRenderer::new(total_bytes);
            let start = Instant::now();

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
        Some(s) => Some(s.snapshot()),
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
    progress_bar: ProgressBar,
}

impl TextRenderer {
    fn new(total_bytes: Option<u64>) -> Self {
        let pb = ProgressBar::new(total_bytes.unwrap_or(0));
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec}, {eta})")
                .unwrap()
                .progress_chars("#>-"),
        );
        Self { progress_bar: pb }
    }

    fn render(&mut self, snapshot: &ProgressSnapshot, finish: Option<ProgressFinish>) {
        self.progress_bar.set_position(snapshot.downloaded);
        if let Some(finish) = finish {
            match finish {
                ProgressFinish::Success => self.progress_bar.finish_with_message("Download complete".green().to_string()),
                ProgressFinish::Failure => self.progress_bar.finish_with_message("Download failed".red().to_string()),
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
        let fraction = snapshot.total.map(|total| if total > 0 { snapshot.downloaded as f64 / total as f64 } else { 1.0 });
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
