use std::convert::TryFrom;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use clap::{ArgAction, Parser};
use reqwest::Url;

use crate::checksum::ChecksumSpec;
use crate::download::{DownloadConfig, ProgressMode};
use crate::util::{infer_output_path, parse_bandwidth_limit};

#[derive(Parser, Debug, Clone)]
#[command(name = "kdownload", author, version, about = "Blazing-fast command-line downloader", long_about = None)]
pub struct Cli {
    /// Primary download URL(s). Additional URLs act as mirrors.
    #[arg(value_name = "url", required = true)]
    pub urls: Vec<String>,

    /// Output file or directory
    #[arg(short, long, value_name = "path")]
    pub output: Option<PathBuf>,

    /// Maximum connections per host
    #[arg(
        short = 'c',
        long = "connections",
        value_name = "int",
        default_value_t = 32
    )]
    pub connections: usize,

    /// Initial number of segments
    #[arg(
        short = 's',
        long = "segments",
        value_name = "int",
        default_value_t = 64
    )]
    pub segments: usize,

    /// Register additional mirrors
    #[arg(short = 'm', long = "mirror", value_name = "url")]
    pub mirrors: Vec<String>,

    /// Verify SHA256 checksum (hex string or file path)
    #[arg(long = "sha256", value_name = "hex|path")]
    pub sha256: Option<String>,

    /// Resume from existing partial download
    #[arg(long = "resume", action = ArgAction::SetTrue)]
    pub resume: bool,

    /// Per-request timeout in seconds
    #[arg(long = "timeout", value_name = "secs")]
    pub timeout: Option<u64>,

    /// Limit bandwidth (e.g. 50M/s)
    #[arg(long = "bandwidth-limit", value_name = "rate")]
    pub bandwidth_limit: Option<String>,

    /// Allow more than 32 connections (advanced)
    #[arg(long = "unsafe-conn", value_name = "int")]
    pub unsafe_conn: Option<usize>,

    /// Quiet mode
    #[arg(short = 'q', long = "quiet", action = ArgAction::SetTrue, conflicts_with = "verbose")]
    pub quiet: bool,

    /// Verbose mode
    #[arg(short = 'v', long = "verbose", action = ArgAction::SetTrue)]
    pub verbose: bool,

    /// Stream progress as newline-delimited JSON
    #[arg(long = "json", action = ArgAction::SetTrue)]
    pub json: bool,
}

impl Cli {
    pub fn parse() -> Self {
        <Self as Parser>::parse()
    }
}

impl TryFrom<Cli> for DownloadConfig {
    type Error = anyhow::Error;

    fn try_from(cli: Cli) -> Result<Self> {
        if cli.urls.is_empty() {
            return Err(anyhow!("at least one URL is required"));
        }

        let mut all_urls = vec![];
        for url in cli.urls.iter().chain(cli.mirrors.iter()) {
            let parsed = Url::parse(url).with_context(|| format!("invalid URL: {url}"))?;
            if parsed.scheme() != "http" && parsed.scheme() != "https" {
                return Err(anyhow!("unsupported URL scheme: {}", parsed.scheme()));
            }
            all_urls.push(parsed);
        }

        let allow_unsafe = cli.unsafe_conn.unwrap_or(64);
        let max_per_host = if cli.unsafe_conn.is_some() {
            cli.connections.max(1)
        } else {
            cli.connections.min(64).max(1)
        };
        if cli.unsafe_conn.is_some() && cli.connections > allow_unsafe {
            return Err(anyhow!(
                "--connections exceeds unsafe limit; either lower it or raise --unsafe-conn"
            ));
        }

        let output = infer_output_path(cli.output.clone(), &all_urls)?;
        let partmap_path = crate::util::derive_partmap_path(&output);

        let timeout = cli.timeout.map(Duration::from_secs);
        let bandwidth_limit = if let Some(limit) = cli.bandwidth_limit.clone() {
            Some(parse_bandwidth_limit(&limit)?)
        } else {
            None
        };

        let sha256 = if let Some(value) = cli.sha256.clone() {
            Some(ChecksumSpec::from_input(&value)?)
        } else {
            None
        };

        let progress = if cli.json {
            ProgressMode::Json
        } else if cli.quiet {
            ProgressMode::Quiet
        } else {
            ProgressMode::Text
        };

        Ok(DownloadConfig {
            urls: all_urls,
            output_path: output,
            partmap_path,
            resume: cli.resume,
            initial_segments: cli.segments.max(1),
            max_connections_per_host: max_per_host,
            unsafe_connection_cap: allow_unsafe,
            timeout,
            bandwidth_limit,
            expected_sha256: sha256,
            progress,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_mode_defaults_to_text() {
        let cli =
            Cli::try_parse_from(["kdownload", "https://example.com/file"]).expect("cli parse");
        let config = DownloadConfig::try_from(cli).expect("config");
        assert_eq!(config.progress, ProgressMode::Text);
    }

    #[test]
    fn progress_mode_respects_quiet() {
        let cli = Cli::try_parse_from(["kdownload", "https://example.com/file", "--quiet"])
            .expect("cli parse");
        let config = DownloadConfig::try_from(cli).expect("config");
        assert_eq!(config.progress, ProgressMode::Quiet);
    }

    #[test]
    fn progress_mode_prefers_json_flag() {
        let cli =
            Cli::try_parse_from(["kdownload", "https://example.com/file", "--quiet", "--json"])
                .expect("cli parse");
        let config = DownloadConfig::try_from(cli).expect("config");
        assert_eq!(config.progress, ProgressMode::Json);
    }
}
