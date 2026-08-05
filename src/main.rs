use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use fuser::MountOption;

/// Mount a directory of RAR-archived videos as a read-only filesystem.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Directory tree containing the RAR archives
    source_dir: PathBuf,
    /// Mount point (must exist and be empty)
    mountpoint: PathBuf,
    /// Write logs to this file instead of stderr
    #[arg(long)]
    log: Option<PathBuf>,
    /// Allow other users (e.g. the plex/jellyfin user) to access the mount.
    /// Requires user_allow_other in /etc/fuse.conf.
    #[arg(long)]
    allow_other: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match &cli.log {
        Some(path) => {
            let file = std::fs::File::create(path).context("create log file")?;
            tracing_subscriber::fmt()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
        }
        None => tracing_subscriber::fmt::init(),
    }
    let source = cli
        .source_dir
        .canonicalize()
        .context("source dir not accessible")?;
    let mut opts = vec![
        MountOption::RO,
        MountOption::FSName("rarfs".into()),
        MountOption::DefaultPermissions,
    ];
    if cli.allow_other {
        opts.push(MountOption::AllowOther);
    }
    tracing::info!("mounting {} at {}", source.display(), cli.mountpoint.display());
    let fs = rarfs::fs::RarFs::new(source);
    fuser::mount2(fs, &cli.mountpoint, &opts).context("mount failed")?;
    Ok(())
}
