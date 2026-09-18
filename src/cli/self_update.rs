use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use super::common::{block_on, confirm, print_json};
use crate::update::{GitHubVersionSource, ensure_binstall_available, run_binstall};

#[derive(Subcommand)]
pub enum UpdateAction {
    /// Check GitHub releases for a newer version
    Check {
        /// Emit machine-readable JSON instead of a text summary
        #[arg(long)]
        json: bool,
    },
    /// Self-update via `cargo binstall` (does not restart any running server —
    /// restart `mynd up`/the background service afterward)
    Apply {
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
}

pub fn cmd_update(action: UpdateAction) -> Result<()> {
    match action {
        UpdateAction::Check { json } => block_on(async move {
            let current = env!("CARGO_PKG_VERSION");
            let source = GitHubVersionSource::new();
            let release = source.latest().await?;
            let is_newer = match (
                semver::Version::parse(current),
                semver::Version::parse(&release.version),
            ) {
                (Ok(cur), Ok(latest)) => latest > cur,
                _ => false,
            };
            if json {
                print_json(&json!({
                    "current_version": current,
                    "latest_version": release.version,
                    "available": is_newer,
                    "release_url": release.html_url,
                }));
            } else if is_newer {
                println!("update available: v{current} -> v{}", release.version);
                println!("{}", release.html_url);
                println!();
                println!("apply it with: mynd update apply");
            } else {
                println!("v{current} is up to date (latest: v{})", release.version);
            }
            Ok(())
        }),
        UpdateAction::Apply { yes } => {
            if !confirm("This will run `cargo binstall oxmynd`. Continue?", yes)? {
                println!("Cancelled.");
                return Ok(());
            }
            block_on(async {
                println!("updating...");
                ensure_binstall_available().await?;
                run_binstall().await?;
                println!(
                    "updated. Restart `mynd up` (or the background service) to run the new version."
                );
                Ok(())
            })
        }
    }
}
