use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use hex::FromHex;
use sha2::{Digest, Sha256};
use tokio::task;

#[derive(Debug, Clone)]
pub struct ChecksumSpec {
    expected: [u8; 32],
    source: String,
}

impl ChecksumSpec {
    pub fn from_input(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(anyhow!("checksum value cannot be empty"));
        }

        if trimmed.len() == 64 && trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            let bytes = <[u8; 32]>::from_hex(trimmed).map_err(|_| anyhow!("invalid hex digest"))?;
            return Ok(Self {
                expected: bytes,
                source: trimmed.to_string(),
            });
        }

        let path = Path::new(trimmed);
        if !path.exists() {
            return Err(anyhow!("checksum file does not exist: {}", trimmed));
        }

        let file = File::open(path)
            .with_context(|| format!("failed to open checksum file {}", trimmed))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|_| anyhow!("failed to read checksum file"))?;
        let token = line
            .split_whitespace()
            .next()
            .ok_or_else(|| anyhow!("checksum file is empty"))?;
        let bytes = <[u8; 32]>::from_hex(token)
            .map_err(|_| anyhow!("invalid hex digest in checksum file"))?;
        Ok(Self {
            expected: bytes,
            source: token.to_string(),
        })
    }

    pub async fn verify_file(&self, path: &Path) -> Result<()> {
        let path_owned = path.to_owned();
        let expected = self.expected;
        let computed = task::spawn_blocking(move || compute_sha256(&path_owned)).await??;
        if computed == expected {
            Ok(())
        } else {
            Err(anyhow!(
                "checksum mismatch: expected {}, got {}",
                hex::encode(expected),
                hex::encode(computed)
            ))
        }
    }

    pub fn display(&self) -> String {
        self.source.clone()
    }
}

fn compute_sha256(path: &Path) -> Result<[u8; 32]> {
    let mut file = File::open(path).with_context(|| format!("failed to open {:?}", path))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let result: [u8; 32] = hasher.finalize().into();
    Ok(result)
}
