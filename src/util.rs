use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use reqwest::Url;

const DEFAULT_FILENAME: &str = "download.bin";

pub fn infer_output_path(provided: Option<PathBuf>, urls: &[Url]) -> Result<PathBuf> {
    let primary = urls
        .first()
        .ok_or_else(|| anyhow!("at least one URL is required to infer output"))?;

    match provided {
        Some(path) => {
            if path.exists() {
                if path.is_dir() {
                    let filename = filename_from_url(primary);
                    return Ok(path.join(filename));
                }
                return Ok(path);
            }

            let looks_like_dir = path
                .to_str()
                .map(|s| s.ends_with(std::path::MAIN_SEPARATOR))
                .unwrap_or(false);

            if looks_like_dir {
                fs::create_dir_all(&path)
                    .with_context(|| format!("failed to create directory {:?}", path))?;
                let filename = filename_from_url(primary);
                return Ok(path.join(filename));
            }

            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() && !parent.exists() {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("failed to create parent directory {:?}", parent)
                    })?;
                }
            }
            Ok(path)
        }
        None => {
            let filename = filename_from_url(primary);
            Ok(PathBuf::from(filename))
        }
    }
}

fn filename_from_url(url: &Url) -> String {
    url.path_segments()
        .and_then(|segments| {
            segments
                .filter(|s| !s.is_empty())
                .last()
                .map(|s| s.to_string())
        })
        .filter(|name| !name.ends_with('/'))
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_FILENAME.to_string())
}

pub fn derive_partmap_path(output: &Path) -> PathBuf {
    let mut name = output
        .file_name()
        .map(|os| os.to_os_string())
        .unwrap_or_else(|| DEFAULT_FILENAME.into());
    name.push(".kdl.partmap");
    output.with_file_name(name)
}

pub fn parse_bandwidth_limit(input: &str) -> Result<u64> {
    let normalized = input
        .trim()
        .trim_end_matches("/s")
        .trim_end_matches("ps")
        .trim();
    if normalized.is_empty() {
        return Err(anyhow!("bandwidth limit cannot be empty"));
    }

    let mut number_part = String::new();
    let mut suffix_part = String::new();
    for ch in normalized.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            number_part.push(ch);
        } else {
            suffix_part.push(ch);
        }
    }

    let value: f64 = number_part
        .parse()
        .map_err(|_| anyhow!("invalid numeric value in bandwidth limit: {normalized}"))?;

    let multiplier = match suffix_part.trim().to_ascii_lowercase().as_str() {
        "" => 1.0,
        "k" => 1_000.0,
        "kb" => 1_000.0,
        "ki" | "kib" => 1024.0,
        "m" => 1_000_000.0,
        "mb" => 1_000_000.0,
        "mi" | "mib" => 1_048_576.0,
        "g" => 1_000_000_000.0,
        "gb" => 1_000_000_000.0,
        "gi" | "gib" => 1_073_741_824.0,
        other => return Err(anyhow!("unsupported bandwidth suffix: {other}")),
    };

    let bytes_per_sec = (value * multiplier).round();
    if bytes_per_sec <= 0.0 {
        return Err(anyhow!("bandwidth limit must be positive"));
    }

    Ok(bytes_per_sec as u64)
}

pub fn format_bytes(value: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut val = value as f64;
    let mut unit = 0usize;
    while val >= 1024.0 && unit < UNITS.len() - 1 {
        val /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", value, UNITS[unit])
    } else {
        format!("{val:.2} {}", UNITS[unit])
    }
}

pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create directory {:?}", parent))?;
        }
    }
    Ok(())
}
