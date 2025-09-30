mod checksum;
mod cli;
mod download;
mod scheduler;
mod util;

use anyhow::Result;
use cli::Cli;
use download::{DownloadConfig, DownloadManager};
use log::{debug, error, info};

#[tokio::main]
async fn main() {
    if let Err(err) = run().await {
        error!("{err}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    let cli = Cli::parse();
    init_logger(&cli);

    debug!("CLI arguments: {:?}", cli);
    let config: DownloadConfig = cli.try_into()?;

    let manager = DownloadManager::new(config)?;
    manager.run().await?;

    info!("Download completed successfully");
    Ok(())
}

fn init_logger(cli: &Cli) {
    use env_logger::Env;
    use log::LevelFilter;

    let mut builder = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
    let level = if cli.quiet {
        LevelFilter::Error
    } else if cli.verbose {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };
    builder.filter_level(level);
    // keep logs quiet unless verbose
    if !cli.verbose {
        builder.format_timestamp_secs();
    }
    let _ = builder.try_init();
}
