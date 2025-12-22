use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;

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

#[derive(Serialize, Deserialize)]
struct SegmentUpdate {
    id: usize,
    downloaded: u64,
}

struct PartMapState {
    map: PartMap,
    file: File,
}

pub struct PartMapHandle {
    path: PathBuf,
    state: Mutex<PartMapState>,
}

impl PartMapHandle {
    pub async fn load_or_create(path: PathBuf, file_size: u64, chunk_size: u64) -> Result<Self> {
        if path.exists() {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .await
                .with_context(|| format!("failed to open part map {:?}", path))?;

            let mut data = Vec::new();
            file.read_to_end(&mut data).await?;

            if !data.is_empty() {
                // Try to deserialize the base map
                let mut offset = 0;
                match bincode::deserialize::<PartMap>(&data) {
                    Ok(mut map) => {
                        offset += bincode::serialized_size(&map)? as usize;
                        
                        // Check if valid
                        if map.file_size == file_size {
                             // Replay updates
                             while offset < data.len() {
                                 match bincode::deserialize::<SegmentUpdate>(&data[offset..]) {
                                     Ok(update) => {
                                         if let Some(seg) = map.segments.get_mut(update.id) {
                                             seg.downloaded = update.downloaded;
                                         }
                                         offset += bincode::serialized_size(&update)? as usize;
                                     }
                                     Err(_) => break, // Stop on partial/corrupt update
                                 }
                             }
                             
                             // Re-open in append mode
                             let file = OpenOptions::new()
                                .append(true)
                                .open(&path)
                                .await?;

                             return Ok(Self {
                                 path,
                                 state: Mutex::new(PartMapState { map, file }),
                             });
                        }
                    }
                    Err(_) => {
                        // Invalid format, ignore and overwrite
                    }
                }
            }
        }

        // Create new
        let map = PartMap::new(file_size, chunk_size);
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)
            .await?;
        
        let bytes = bincode::serialize(&map)?;
        file.write_all(&bytes).await?;

        Ok(Self {
            path,
            state: Mutex::new(PartMapState { map, file }),
        })
    }

    pub async fn segments(&self) -> Vec<PartSegment> {
        self.state.lock().await.map.segments.clone()
    }

    pub async fn record_progress(
        &self,
        id: usize,
        downloaded: u64,
        _force_flush: bool,
    ) -> Result<()> {
        let mut state = self.state.lock().await;
        let segment = state
            .map
            .segments
            .iter_mut()
            .find(|seg| seg.id == id)
            .ok_or_else(|| anyhow!("segment {id} not found in part map"))?;
        
        segment.downloaded = downloaded.min(segment.len());
        
        let update = SegmentUpdate {
            id,
            downloaded: segment.downloaded,
        };
        let bytes = bincode::serialize(&update)?;
        state.file.write_all(&bytes).await?;
        
        // We rely on OS buffering and occasional syncs by the user or OS.
        // If we want durability, we could sync_data periodically, but speed is priority here.
        Ok(())
    }

    pub async fn segment(&self, id: usize) -> Option<PartSegment> {
        let state = self.state.lock().await;
        state.map.segments.iter().find(|seg| seg.id == id).cloned()
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
