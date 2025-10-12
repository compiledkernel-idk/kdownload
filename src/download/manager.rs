use crate::download::bandwidth::BandwidthLimiter;
use crate::download::mirror::MirrorPool;
use crate::download::partmap::PartMapHandle;
use crate::download::DownloadConfig;
use crate::progress::{ProgressFinish, ProgressReporter};
use crate::scheduler::{Scheduler, SegmentStats, SegmentTask};
use crate::util::{ensure_parent_dir, format_bytes};

use anyhow::{anyhow, Context, Result};
use futures_util::StreamExt;
use log::{debug, info, warn};
use reqwest::{header, Client, StatusCode, Url};
use std::fs::{File, OpenOptions};
use std::io::{self, Seek, SeekFrom, Write};
#[cfg(unix)]
use std::os::unix::fs::FileExt;
#[cfg(windows)]
use std::os::windows::fs::FileExt as WindowsFileExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs as async_fs;
use tokio::task::JoinSet;
use tokio::time::sleep;

#[cfg(target_os = "linux")]
use nix::errno::Errno;
#[cfg(target_os = "linux")]
use nix::fcntl::{fallocate, FallocateFlags};
#[cfg(target_os = "linux")]
use std::os::unix::io::AsRawFd;

const MIN_CHUNK_SIZE: u64 = 4 << 20; // 4 MiB (increased from 1 MiB)
const MAX_RETRIES: usize = 5;
const WRITE_BUFFER_SIZE: usize = 512 << 10; // 512 KiB write buffer

pub struct DownloadManager {
    config: DownloadConfig,
    client: Client,
    mirrors: MirrorPool,
    bandwidth: Option<Arc<BandwidthLimiter>>,
}

struct FileMetadata {
    content_length: Option<u64>,
    supports_ranges: bool,
    filename: Option<String>,
}

enum SegmentOutcome {
    Completed(SegmentStats),
    Failed(anyhow::Error),
}

impl DownloadManager {
    pub fn new(config: DownloadConfig) -> Result<Self> {
        let mirrors = MirrorPool::new(config.urls.clone());
        let mut builder = Client::builder()
            .user_agent("kdownload/0.1")
            .redirect(reqwest::redirect::Policy::limited(10))
            .pool_max_idle_per_host(config.max_connections_per_host)
            .pool_idle_timeout(Some(std::time::Duration::from_secs(90)))
            .tcp_nodelay(true)
            .http2_adaptive_window(true)
            .http2_keep_alive_interval(Some(std::time::Duration::from_secs(10)))
            .http2_keep_alive_timeout(std::time::Duration::from_secs(20));
        if let Some(timeout) = config.timeout {
            builder = builder.timeout(timeout);
        }
        let client = builder.build().context("failed to build HTTP client")?;
        let bandwidth = config
            .bandwidth_limit
            .map(|limit| Arc::new(BandwidthLimiter::new(limit)));
        Ok(Self {
            config,
            client,
            mirrors,
            bandwidth,
        })
    }

    pub async fn run(self) -> Result<()> {
        ensure_parent_dir(&self.config.output_path)?;
        let metadata = self.probe_metadata().await?;
        let file_path = self.config.output_path.clone();
        if file_path.exists() && !self.config.resume {
            return Err(anyhow!(
                "output file {:?} already exists; use --resume to continue",
                file_path
            ));
        }

        if metadata.supports_ranges && metadata.content_length.is_some() {
            self.download_segments(metadata).await?;
        } else {
            warn!("server does not support ranged requests; falling back to single connection");
            self.download_streaming(metadata).await?;
        }

        if let Some(spec) = &self.config.expected_sha256 {
            info!("verifying SHA256 checksum ({})", spec.display());
            spec.verify_file(&self.config.output_path).await?;
        }

        Ok(())
    }

    async fn probe_metadata(&self) -> Result<FileMetadata> {
        for url in self.mirrors.all() {
            match self.try_head(&url).await {
                Ok(meta) => return Ok(meta),
                Err(err) => {
                    debug!("HEAD request failed for {}: {err}", url);
                    continue;
                }
            }
        }
        Err(anyhow!("failed to retrieve metadata from all mirrors"))
    }

    async fn try_head(&self, url: &Url) -> Result<FileMetadata> {
        let response = self.client.head(url.clone()).send().await?;
        if response.status().is_success() {
            let length = parse_content_length(response.headers().get(header::CONTENT_LENGTH));
            let supports_ranges = response
                .headers()
                .get(header::ACCEPT_RANGES)
                .and_then(|v| v.to_str().ok())
                .map(|v| v.to_ascii_lowercase().contains("bytes"))
                .unwrap_or(false);
            let filename = filename_from_headers(&response);
            if length.is_some() {
                return Ok(FileMetadata {
                    content_length: length,
                    supports_ranges,
                    filename,
                });
            }

            if supports_ranges {
                let mut meta = self.try_range_probe(url).await?;
                if meta.filename.is_none() {
                    meta.filename = filename;
                }
                return Ok(meta);
            }

            Ok(FileMetadata {
                content_length: length,
                supports_ranges,
                filename,
            })
        } else if matches!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
        ) {
            self.try_range_probe(url).await
        } else {
            Err(anyhow!("{} returned status {}", url, response.status()))
        }
    }

    async fn try_range_probe(&self, url: &Url) -> Result<FileMetadata> {
        let response = self
            .client
            .get(url.clone())
            .header(header::RANGE, "bytes=0-0")
            .send()
            .await?;

        if response.status() == StatusCode::PARTIAL_CONTENT {
            let total = parse_content_range(response.headers().get(header::CONTENT_RANGE))
                .ok_or_else(|| anyhow!("missing Content-Range header"))?;
            let filename = filename_from_headers(&response);
            let _ = response.bytes().await?; // consume body
            Ok(FileMetadata {
                content_length: Some(total),
                supports_ranges: true,
                filename,
            })
        } else if response.status().is_success() {
            let filename = filename_from_headers(&response);
            let length = response.content_length();
            let _ = response.bytes().await?;
            Ok(FileMetadata {
                content_length: length,
                supports_ranges: false,
                filename,
            })
        } else {
            Err(anyhow!(
                "range probe for {} returned status {}",
                url,
                response.status()
            ))
        }
    }

    async fn download_segments(&self, metadata: FileMetadata) -> Result<()> {
        let total_size = metadata
            .content_length
            .ok_or_else(|| anyhow!("content length is required for segmented download"))?;
        let chunk_size = compute_chunk_size(total_size, self.config.initial_segments);

        let file = prepare_output_file(&self.config.output_path, total_size, self.config.resume)?;
        let file = Arc::new(file);

        let partmap =
            PartMapHandle::load_or_create(self.config.partmap_path.clone(), total_size, chunk_size)
                .await?;
        let partmap = Arc::new(partmap);

        let segments = partmap.segments().await;
        let total_completed: u64 = segments
            .iter()
            .map(|segment| segment.downloaded.min(segment.len()))
            .sum();

        let mut pending: Vec<SegmentTask> = segments
            .iter()
            .filter(|segment| segment.remaining() > 0)
            .map(|segment| SegmentTask {
                id: segment.id,
                start: segment.start,
                end: segment.end,
                downloaded: segment.downloaded,
            })
            .collect();

        if pending.is_empty() {
            info!("all segments already downloaded; finalizing");
            partmap.finalize().await?;
            file.sync_all()?;
            return Ok(());
        }

        pending.sort_by_key(|s| s.start);

        let progress = Arc::new(AtomicU64::new(total_completed));
        if total_completed > 0 {
            info!(
                "resuming: {} of {} already present",
                format_bytes(total_completed),
                format_bytes(total_size)
            );
        }

        let initial_parallelism = self
            .config
            .initial_segments
            .min(self.config.max_parallelism())
            .max(1);
        let scheduler = Arc::new(Scheduler::new(
            pending,
            initial_parallelism,
            self.config.max_parallelism(),
        ));

        let mut progress_display = ProgressReporter::spawn(
            self.config.progress,
            Some(total_size),
            total_completed,
            progress.clone(),
            Some(scheduler.clone()),
        );

        let client = self.client.clone();
        let mirrors = self.mirrors.clone();
        let bandwidth = self.bandwidth.clone();
        let mut join_set: JoinSet<SegmentOutcome> = JoinSet::new();

        while scheduler.has_remaining().await {
            while let Some(segment) = scheduler.next_segment().await {
                let client = client.clone();
                let mirrors = mirrors.clone();
                let file = file.clone();
                let partmap = partmap.clone();
                let bandwidth = bandwidth.clone();
                let progress = progress.clone();
                join_set.spawn(async move {
                    match download_segment_with_retry(
                        client,
                        mirrors,
                        file,
                        partmap,
                        bandwidth,
                        progress,
                        segment.clone(),
                    )
                    .await
                    {
                        Ok(stats) => SegmentOutcome::Completed(stats),
                        Err(err) => SegmentOutcome::Failed(err),
                    }
                });
            }

            match join_set.join_next().await {
                Some(Ok(SegmentOutcome::Completed(stats))) => {
                    let segment_id = stats.id;
                    let segment_bytes = stats.bytes;
                    let segment_duration = stats.duration;
                    scheduler.on_segment_complete(stats).await;
                    debug!(
                        "segment {segment_id} completed: {} in {:?}",
                        format_bytes(segment_bytes),
                        segment_duration
                    );
                }
                Some(Ok(SegmentOutcome::Failed(err))) => {
                    Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
                    return Err(err);
                }
                Some(Err(join_err)) => {
                    Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
                    return Err(anyhow!("segment task panic: {}", join_err));
                }
                None => break,
            }
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(SegmentOutcome::Completed(stats)) => {
                    scheduler.on_segment_complete(stats).await;
                }
                Ok(SegmentOutcome::Failed(err)) => {
                    Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
                    return Err(err);
                }
                Err(join_err) => {
                    Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
                    return Err(anyhow!("segment task panic: {}", join_err));
                }
            }
        }

        if let Err(err) = partmap.finalize().await {
            Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
            return Err(err);
        }
        if let Err(err) = file.sync_all() {
            Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
            return Err(err.into());
        }

        Self::finalize_progress(&mut progress_display, ProgressFinish::Success).await;
        if self.config.partmap_path.exists() {
            async_fs::remove_file(&self.config.partmap_path).await.ok();
        }
        Ok(())
    }

    async fn download_streaming(&self, metadata: FileMetadata) -> Result<()> {
        if self.config.partmap_path.exists() {
            async_fs::remove_file(&self.config.partmap_path).await.ok();
        }

        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .read(true)
            .open(&self.config.output_path)
            .with_context(|| format!("failed to open {:?}", self.config.output_path))?;

        let mut start_offset = 0u64;
        let can_resume = self.config.resume && metadata.supports_ranges;

        if can_resume {
            if let Ok(meta) = file.metadata() {
                start_offset = meta.len();
            }
            if start_offset > 0 {
                info!("resuming from byte {start_offset}");
            }
        } else {
            if self.config.resume {
                warn!("server does not allow resume; restarting download");
            }
            file.set_len(0)?;
        }

        file.seek(SeekFrom::Start(start_offset))?;

        let mut request = self.client.get(self.mirrors.primary());
        if can_resume && start_offset > 0 {
            request = request.header(header::RANGE, format!("bytes={}-", start_offset));
        }

        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(anyhow!("download failed with status {}", response.status()));
        }

        let bandwidth = self.bandwidth.clone();
        let progress = Arc::new(AtomicU64::new(start_offset));
        let mut progress_display = ProgressReporter::spawn(
            self.config.progress,
            metadata.content_length,
            start_offset,
            progress.clone(),
            None,
        );

        let mut stream = response.bytes_stream();
        let result: Result<()> = async {
            while let Some(chunk) = stream.next().await {
                let chunk = chunk?;
                if let Some(limiter) = &bandwidth {
                    limiter.consume(chunk.len()).await;
                }
                file.write_all(chunk.as_ref())?;
                progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
            }
            file.sync_all()?;
            Ok(())
        }
        .await;

        match result {
            Ok(()) => {
                Self::finalize_progress(&mut progress_display, ProgressFinish::Success).await;
                Ok(())
            }
            Err(err) => {
                Self::finalize_progress(&mut progress_display, ProgressFinish::Failure).await;
                Err(err)
            }
        }
    }

    async fn finalize_progress(progress: &mut Option<ProgressReporter>, finish: ProgressFinish) {
        if let Some(reporter) = progress.take() {
            reporter.finish(finish).await;
        }
    }
}

#[cfg(unix)]
fn write_all_at(file: &File, buf: &[u8], position: u64) -> io::Result<()> {
    file.write_all_at(buf, position)
}

#[cfg(windows)]
fn write_all_at(file: &File, mut buf: &[u8], mut position: u64) -> io::Result<()> {
    while !buf.is_empty() {
        let written = file.seek_write(buf, position)?;
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "failed to write segment data",
            ));
        }
        buf = &buf[written..];
        position += written as u64;
    }
    Ok(())
}

fn parse_content_length(value: Option<&header::HeaderValue>) -> Option<u64> {
    value
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse().ok())
}

fn parse_content_range(value: Option<&header::HeaderValue>) -> Option<u64> {
    let raw = value?.to_str().ok()?;
    let parts: Vec<&str> = raw.split('/').collect();
    if parts.len() != 2 {
        return None;
    }
    parts[1].parse().ok()
}

fn filename_from_headers(response: &reqwest::Response) -> Option<String> {
    response
        .headers()
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_content_disposition)
}

fn parse_content_disposition(value: &str) -> Option<String> {
    for part in value.split(';') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("filename=") {
            let trimmed = rest.trim_matches('"');
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn compute_chunk_size(total: u64, initial_segments: usize) -> u64 {
    if total == 0 {
        return 1;
    }
    let segments = initial_segments.max(1) as u64;
    let base = (total + segments - 1) / segments;
    base.max(MIN_CHUNK_SIZE).min(total)
}

fn prepare_output_file(path: &PathBuf, size: u64, resume: bool) -> Result<File> {
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .read(true)
        .open(path)
        .with_context(|| format!("failed to open {:?}", path))?;

    if !resume || file.metadata()?.len() < size {
        preallocate(&file, size)?;
    }
    Ok(file)
}

fn preallocate(file: &File, size: u64) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        if size > 0 {
            if let Err(err) = fallocate(
                file.as_raw_fd(),
                FallocateFlags::FALLOC_FL_KEEP_SIZE,
                0,
                size as i64,
            ) {
                if err != Errno::ENOTSUP && err != Errno::EINVAL {
                    return Err(anyhow!("fallocate failed: {err}"));
                }
            }
        }
        file.set_len(size)?;
        return Ok(());
    }

    #[cfg(not(target_os = "linux"))]
    {
        file.set_len(size)?;
        Ok(())
    }
}

async fn download_segment_with_retry(
    client: Client,
    mirrors: MirrorPool,
    file: Arc<File>,
    partmap: Arc<PartMapHandle>,
    bandwidth: Option<Arc<BandwidthLimiter>>,
    progress: Arc<AtomicU64>,
    segment: SegmentTask,
) -> Result<SegmentStats> {
    if segment.remaining_range().is_none() {
        return Ok(SegmentStats {
            id: segment.id,
            bytes: 0,
            duration: Duration::from_secs(0),
        });
    }

    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match download_segment_once(
            client.clone(),
            mirrors.clone(),
            file.clone(),
            partmap.clone(),
            bandwidth.clone(),
            progress.clone(),
            segment.clone(),
        )
        .await
        {
            Ok(stats) => return Ok(stats),
            Err(err) if attempt < MAX_RETRIES => {
                warn!(
                    "segment {} failed on attempt {}: {err}; retrying",
                    segment.id, attempt
                );
                sleep(Duration::from_secs(1 << attempt.min(4))).await;
            }
            Err(err) => return Err(err),
        }
    }
}

async fn download_segment_once(
    client: Client,
    mirrors: MirrorPool,
    file: Arc<File>,
    partmap: Arc<PartMapHandle>,
    bandwidth: Option<Arc<BandwidthLimiter>>,
    progress: Arc<AtomicU64>,
    segment: SegmentTask,
) -> Result<SegmentStats> {
    let segment_state = partmap
        .segment(segment.id)
        .await
        .ok_or_else(|| anyhow!("segment {} missing in part map", segment.id))?;

    if segment_state.remaining() == 0 {
        return Ok(SegmentStats {
            id: segment.id,
            bytes: 0,
            duration: Duration::from_secs(0),
        });
    }

    let mut position = segment_state.start + segment_state.downloaded;
    let end = segment_state.end;

    let mut builder = client.get(mirrors.next());
    builder = builder.header(header::RANGE, format!("bytes={}-{}", position, end));

    let start_time = Instant::now();
    let response = builder.send().await?;
    if !(response.status() == StatusCode::PARTIAL_CONTENT
        || (position == 0 && response.status().is_success()))
    {
        return Err(anyhow!(
            "unexpected status {} for segment {}",
            response.status(),
            segment.id
        ));
    }

    let mut downloaded = segment_state.downloaded;
    let mut total_downloaded = 0u64;
    let mut write_buffer = Vec::with_capacity(WRITE_BUFFER_SIZE);
    let mut buffer_position = position;

    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if let Some(limiter) = &bandwidth {
            limiter.consume(chunk.len()).await;
        }

        // Buffer writes to reduce syscalls
        write_buffer.extend_from_slice(&chunk);

        if write_buffer.len() >= WRITE_BUFFER_SIZE {
            write_all_at(&file, &write_buffer, buffer_position)?;
            buffer_position += write_buffer.len() as u64;
            write_buffer.clear();
        }

        position += chunk.len() as u64;
        downloaded += chunk.len() as u64;
        total_downloaded += chunk.len() as u64;
        progress.fetch_add(chunk.len() as u64, Ordering::Relaxed);
    }

    // Flush remaining buffered data
    if !write_buffer.is_empty() {
        write_all_at(&file, &write_buffer, buffer_position)?;
    }

    let completed = downloaded >= segment.len();
    partmap
        .record_progress(segment.id, downloaded, completed)
        .await?;

    Ok(SegmentStats {
        id: segment.id,
        bytes: total_downloaded,
        duration: start_time.elapsed(),
    })
}
