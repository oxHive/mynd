use anyhow::Result;
use serde_json::json;

use super::common::{block_on, confirm, print_json};
use crate::update::{
    GitHubVersionSource, InstallMethod, ScriptOutput, is_newer_than_current, run_install_script,
};

/// `mynd update`: check GitHub releases for a newer version. Never installs
/// anything — that's `mynd upgrade`.
pub fn cmd_update(json: bool) -> Result<()> {
    block_on(async move {
        let current = env!("CARGO_PKG_VERSION");
        let release = GitHubVersionSource::new().latest().await?;
        let is_newer = is_newer_than_current(&release.version);
        let method = InstallMethod::detect();
        if json {
            print_json(&json!({
                "current_version": current,
                "latest_version": release.version,
                "available": is_newer,
                "release_url": release.html_url,
                "install_method": method,
                "upgrade_command": method.upgrade_hint(),
            }));
        } else if is_newer {
            println!("update available: v{current} -> v{}", release.version);
            println!("{}", release.html_url);
            println!();
            println!("upgrade with: {}", method.upgrade_hint());
        } else {
            println!("v{current} is up to date (latest: v{})", release.version);
        }
        Ok(())
    })
}

/// `mynd upgrade`: replace this binary with the latest release by re-running
/// the install script. Only for script installs — Homebrew and cargo own
/// their binaries, so for those this prints the right command instead.
pub fn cmd_upgrade(yes: bool) -> Result<()> {
    let method = InstallMethod::detect();
    let InstallMethod::Script { install_dir } = &method else {
        println!("{}", method.refusal_message());
        return Ok(());
    };

    block_on(async {
        let current = env!("CARGO_PKG_VERSION");
        let release = GitHubVersionSource::new().latest().await?;
        if !is_newer_than_current(&release.version) {
            println!("v{current} is up to date (latest: v{})", release.version);
            return Ok(());
        }
        let prompt = format!(
            "Upgrade mynd v{current} -> v{} in {}?",
            release.version,
            install_dir.display()
        );
        if !confirm(&prompt, yes)? {
            println!("Cancelled.");
            return Ok(());
        }
        run_install_script(install_dir, ScriptOutput::Inherit).await?;
        println!("upgraded. Restart `mynd up` (or the background service) to run the new version.");
        Ok(())
    })
}
