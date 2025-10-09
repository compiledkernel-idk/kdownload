mod bandwidth;
mod manager;
mod mirror;
mod partmap;

pub use manager::DownloadManager;

use std::path::PathBuf;
use std::time::Duration;

use reqwest::Url;

use crate::checksum::ChecksumSpec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressMode {
    Quiet,
    Text,
    Json,
}

#[derive(Debug, Clone)]
pub struct DownloadConfig {
    pub urls: Vec<Url>,
    pub output_path: PathBuf,
    pub partmap_path: PathBuf,
    pub resume: bool,
    pub initial_segments: usize,
    pub max_connections_per_host: usize,
    pub unsafe_connection_cap: usize,
    pub timeout: Option<Duration>,
    pub bandwidth_limit: Option<u64>,
    pub expected_sha256: Option<ChecksumSpec>,
    pub progress: ProgressMode,
}

impl DownloadConfig {
    pub fn max_parallelism(&self) -> usize {
        self.max_connections_per_host
            .min(self.unsafe_connection_cap)
            .max(1)
    }
}
