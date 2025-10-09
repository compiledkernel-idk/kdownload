use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs;
use tokio::sync::Mutex;

const FLUSH_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartSegment {
    pub id: usize,
    pub start: u64,
    pub end: u64,
    pub downloaded: u64,
}

impl PartSegment {
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }

    pub fn remaining(&self) -> u64 {
        self.len().saturating_sub(self.downloaded)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartMap {
    pub file_size: u64,
    pub chunk_size: u64,
    pub segments: Vec<PartSegment>,
}

impl PartMap {
    pub fn new(file_size: u64, chunk_size: u64) -> Self {
        let chunk_size = chunk_size.max(1);
        let mut segments = Vec::new();
        if file_size == 0 {
            segments.push(PartSegment {
                id: 0,
                start: 0,
                end: 0,
                downloaded: 0,
            });
            return Self {
                file_size,
                chunk_size,
                segments,
            };
        }

        let mut start = 0u64;
        let mut id = 0usize;
        while start < file_size {
            let end = (start + chunk_size - 1).min(file_size - 1);
            segments.push(PartSegment {
                id,
                start,
                end,
                downloaded: 0,
            });
            start = end.saturating_add(1);
            id += 1;
        }

        Self {
            file_size,
            chunk_size,
            segments,
        }
    }
}

struct PartMapState {
    map: PartMap,
    dirty: bool,
    last_flush: Instant,
}

pub struct PartMapHandle {
    path: PathBuf,
    state: Mutex<PartMapState>,
}

impl PartMapHandle {
    pub async fn load_or_create(path: PathBuf, file_size: u64, chunk_size: u64) -> Result<Self> {
        if path.exists() {
            let bytes = fs::read(&path)
                .await
                .with_context(|| format!("failed to read part map {:?}", path))?;
            let map: PartMap = serde_json::from_slice(&bytes)
                .with_context(|| format!("invalid part map format in {:?}", path))?;
            if map.file_size != file_size {
                return Err(anyhow!(
                    "part map file size mismatch (expected {}, found {})",
                    file_size,
                    map.file_size
                ));
            }
            return Ok(Self {
                path,
                state: Mutex::new(PartMapState {
                    map,
                    dirty: false,
                    last_flush: Instant::now(),
                }),
            });
        }

        let map = PartMap::new(file_size, chunk_size);
        let handle = Self {
            path: path.clone(),
            state: Mutex::new(PartMapState {
                map,
                dirty: true,
                last_flush: Instant::now(),
            }),
        };
        handle.persist().await?;
        Ok(handle)
    }

    pub async fn segments(&self) -> Vec<PartSegment> {
        self.state.lock().await.map.segments.clone()
    }

    pub async fn record_progress(
        &self,
        id: usize,
        downloaded: u64,
        force_flush: bool,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let segment = state
            .map
            .segments
            .iter_mut()
            .find(|seg| seg.id == id)
            .ok_or_else(|| anyhow!("segment {id} not found in part map"))?;
        segment.downloaded = downloaded.min(segment.len());
        state.dirty = true;

        let should_flush = force_flush || state.last_flush.elapsed() >= FLUSH_INTERVAL;
        if should_flush {
            Self::flush_locked(&self.path, &mut state).await?;
        }
        Ok(())
    }

    pub async fn segment(&self, id: usize) -> Option<PartSegment> {
        let state = self.state.lock().await;
        state.map.segments.iter().find(|seg| seg.id == id).cloned()
    }

    pub async fn persist(&self) -> Result<()> {
        let mut state = self.state.lock().await;
        Self::flush_locked(&self.path, &mut state).await
    }

    async fn flush_locked(path: &PathBuf, state: &mut PartMapState) -> Result<()> {
        if !state.dirty {
            return Ok(());
        }
        let serialized = serde_json::to_vec_pretty(&state.map)?;
        fs::write(path, serialized)
            .await
            .with_context(|| format!("failed to write part map {:?}", path))?;
        state.dirty = false;
        state.last_flush = Instant::now();
        Ok(())
    }

    pub async fn finalize(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .await
                .with_context(|| format!("failed to remove part map {:?}", self.path))?;
        }
        Ok(())
    }
}
