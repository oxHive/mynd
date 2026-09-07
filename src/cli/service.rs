use super::init::home_dir;
use anyhow::Result;
use std::path::{Path, PathBuf};

// ── service management ────────────────────────────────────────────────────────
// NOTE: systemd unit basenames ("hivemind", "hivemind-matrix"), the launchd
// labels ("com.oxhive.hivemind*") and the log filename ("hivemind.log") are
// still "hivemind" on purpose — renaming them orphans units/agents already
// installed on user machines, so that is deferred to the "Service / daemon"
// rename pass (which needs a migration that uninstalls the old unit first).

pub fn cmd_service_install(dashboard: bool, matrix: bool) -> Result<()> {
    #[cfg(target_os = "macos")]
    return service_install_macos(dashboard, matrix);
    #[cfg(target_os = "linux")]
    return service_install_linux(dashboard, matrix);
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    anyhow::bail!("mynd service install is only supported on Linux and macOS");
}

pub fn cmd_service_uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    return service_uninstall_macos();
    #[cfg(target_os = "linux")]
    return service_uninstall_linux();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    anyhow::bail!("mynd service uninstall is only supported on Linux and macOS");
}

pub fn cmd_service_status() -> Result<()> {
    #[cfg(target_os = "macos")]
    return service_status_macos();
    #[cfg(target_os = "linux")]
    return service_status_linux();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    anyhow::bail!("mynd service status is only supported on Linux and macOS");
}

// ── Linux / systemd user unit ─────────────────────────────────────────────────

#[cfg(target_os = "linux")]
fn systemd_unit_dir() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home_dir().join(".config"))
        .join("systemd")
        .join("user")
}

#[cfg(target_os = "linux")]
fn systemd_unit_path(unit_name: &str) -> PathBuf {
    systemd_unit_dir().join(format!("{unit_name}.service"))
}

#[cfg(target_os = "linux")]
fn systemd_unit_content(description: &str, exe: &Path, exec_args: &[&str]) -> String {
    let mut exec = exe.display().to_string();
    for arg in exec_args {
        exec.push(' ');
        exec.push_str(arg);
    }
    // Carry over the installing shell's PATH — systemd user services start
    // with a bare PATH that omits ~/.cargo/bin, ~/.local/bin, etc., which
    // breaks self-update (it shells out to `cargo binstall`) and any other
    // subprocess the daemon spawns by name.
    let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into());
    format!(
        "[Unit]\n\
         Description={description}\n\
         After=network.target\n\
         \n\
         [Service]\n\
         Type=simple\n\
         Environment=PATH={path}\n\
         ExecStart={exec}\n\
         Restart=on-failure\n\
         RestartSec=5\n\
         \n\
         [Install]\n\
         WantedBy=default.target\n"
    )
}

#[cfg(target_os = "linux")]
fn service_install_unit_linux(
    unit_name: &str,
    description: &str,
    exec_args: &[&str],
) -> Result<()> {
    let exe = std::env::current_exe()?;
    let unit = systemd_unit_content(description, &exe, exec_args);

    let unit_path = systemd_unit_path(unit_name);
    std::fs::create_dir_all(unit_path.parent().unwrap())?;
    std::fs::write(&unit_path, &unit)?;
    println!("Unit file written: {}", unit_path.display());

    // daemon-reload so systemd sees the new unit.
    let reload = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    match reload {
        Ok(s) if s.success() => {}
        _ => {
            println!("Warning: systemctl --user daemon-reload failed — run it manually.");
        }
    }

    let enable = std::process::Command::new("systemctl")
        .args(["--user", "enable", unit_name])
        .status();
    if !matches!(enable, Ok(s) if s.success()) {
        println!("Warning: could not enable {unit_name} automatically.");
        println!("Run: systemctl --user enable {unit_name}");
    }

    // `restart` (rather than `enable --now`) so a unit that's already active
    // with different ExecStart args (e.g. re-running install --dashboard
    // after a headless install) actually picks up the rewritten unit file
    // instead of systemd treating `start` on an active unit as a no-op.
    let restart = std::process::Command::new("systemctl")
        .args(["--user", "restart", unit_name])
        .status();
    match restart {
        Ok(s) if s.success() => {
            println!("{unit_name} service enabled and started.");
        }
        _ => {
            println!("Warning: could not start {unit_name} automatically.");
            println!("Run: systemctl --user restart {unit_name}");
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_uninstall_unit_linux(unit_name: &str) -> Result<()> {
    let _ = std::process::Command::new("systemctl")
        .args(["--user", "disable", "--now", unit_name])
        .status();

    let unit_path = systemd_unit_path(unit_name);
    if unit_path.exists() {
        std::fs::remove_file(&unit_path)?;
        println!("Removed: {}", unit_path.display());
    } else {
        println!("Unit file for {unit_name} not found — nothing to remove.");
    }

    let _ = std::process::Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_status_unit_linux(unit_name: &str) -> Result<()> {
    let status = std::process::Command::new("systemctl")
        .args(["--user", "status", unit_name])
        .status()?;
    if !status.success() {
        anyhow::bail!("{unit_name} is not running or not installed");
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_install_linux(dashboard: bool, matrix: bool) -> Result<()> {
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "Mynd server (API + dashboard)")
    } else {
        (&["up", "--headless"], "Mynd server (API only)")
    };
    service_install_unit_linux("hivemind", desc, args)?;

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `mynd matrix login` first, then re-run `mynd service install --matrix`."
            );
        }
        service_install_unit_linux(
            "hivemind-matrix",
            "Mynd Matrix chat bot",
            &["matrix", "run"],
        )?;
    }

    println!();
    println!("Mynd will now start automatically on login.");
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        println!("Dashboard: http://127.0.0.1:{port}");
    }
    println!("Check status: mynd service status");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_uninstall_linux() -> Result<()> {
    service_uninstall_unit_linux("hivemind")?;
    if systemd_unit_path("hivemind-matrix").exists() {
        service_uninstall_unit_linux("hivemind-matrix")?;
    }

    println!("Mynd service uninstalled.");
    Ok(())
}

#[cfg(target_os = "linux")]
fn service_status_linux() -> Result<()> {
    service_status_unit_linux("hivemind")?;
    if systemd_unit_path("hivemind-matrix").exists() {
        service_status_unit_linux("hivemind-matrix")?;
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod matrix_service_tests {
    use super::*;

    #[test]
    fn systemd_unit_content_for_matrix_names_the_unit_and_subcommand() {
        let content = systemd_unit_content(
            "Mynd Matrix chat bot",
            &std::path::PathBuf::from("/usr/local/bin/mynd"),
            &["matrix", "run"],
        );
        assert!(content.contains("Description=Mynd Matrix chat bot"));
        assert!(content.contains("ExecStart=/usr/local/bin/mynd matrix run"));
        assert!(content.contains("WantedBy=default.target"));
    }

    #[test]
    fn systemd_unit_content_for_up_matches_existing_bare_invocation() {
        // Preserves exact current behavior: the `up` unit has always run the
        // bare binary (stdio MCP mode), zero args — not `up --headless` as
        // one might expect. Not this task's job to change that; just don't
        // silently break it while adding the parameterization.
        let content = systemd_unit_content(
            "Mynd MCP memory server",
            &std::path::PathBuf::from("/usr/local/bin/mynd"),
            &[],
        );
        assert!(content.contains("ExecStart=/usr/local/bin/mynd\n"));
    }
}

// ── macOS / launchd ───────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "com.oxhive.hivemind";

#[cfg(target_os = "macos")]
const MATRIX_LAUNCH_AGENT_LABEL: &str = "com.oxhive.hivemind-matrix";

#[cfg(target_os = "macos")]
fn launch_agent_path(label: &str) -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist"))
}

#[cfg(target_os = "macos")]
fn launch_agent_plist_content(label: &str, exe: &Path, exec_args: &[&str]) -> String {
    let mut program_arguments = format!("<string>{}</string>\n", exe.display());
    for arg in exec_args {
        program_arguments.push_str("             <string>");
        program_arguments.push_str(arg);
        program_arguments.push_str("</string>\n");
    }
    let log_dir = home_dir().join("Library").join("Logs");
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
           <key>Label</key>\n\
           <string>{label}</string>\n\
           <key>ProgramArguments</key>\n\
           <array>\n\
             {program_arguments}\
           </array>\n\
           <key>RunAtLoad</key>\n\
           <true/>\n\
           <key>KeepAlive</key>\n\
           <true/>\n\
           <key>StandardOutPath</key>\n\
           <string>{log_dir}/hivemind.log</string>\n\
           <key>StandardErrorPath</key>\n\
           <string>{log_dir}/hivemind.log</string>\n\
         </dict>\n\
         </plist>\n",
        log_dir = log_dir.display(),
    )
}

#[cfg(target_os = "macos")]
fn service_install_unit_macos(label: &str, exec_args: &[&str], description: &str) -> Result<()> {
    let exe = std::env::current_exe()?;
    let plist_path = launch_agent_path(label);
    let plist = launch_agent_plist_content(label, &exe, exec_args);

    std::fs::create_dir_all(plist_path.parent().unwrap())?;
    std::fs::write(&plist_path, &plist)?;
    println!("Plist written: {}", plist_path.display());

    // Unload first (ignore failure — fine if it wasn't loaded) so re-running
    // install with different args (e.g. --dashboard after a headless install)
    // actually restarts the job with the rewritten plist, instead of
    // `launchctl load` no-op'ing against an already-loaded label.
    let _ = std::process::Command::new("launchctl")
        .args(["unload", "-w", plist_path.to_str().unwrap()])
        .status();

    let load = std::process::Command::new("launchctl")
        .args(["load", "-w", plist_path.to_str().unwrap()])
        .status();
    match load {
        Ok(s) if s.success() => {
            println!("{description} loaded and started.");
        }
        _ => {
            println!("Warning: launchctl load failed — run manually:");
            println!("  launchctl load -w {}", plist_path.display());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall_unit_macos(label: &str) -> Result<()> {
    let plist_path = launch_agent_path(label);

    let _ = std::process::Command::new("launchctl")
        .args(["unload", "-w", plist_path.to_str().unwrap()])
        .status();

    if plist_path.exists() {
        std::fs::remove_file(&plist_path)?;
        println!("Removed: {}", plist_path.display());
    } else {
        println!("Plist for {label} not found — nothing to remove.");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_status_unit_macos(label: &str) -> Result<()> {
    let output = std::process::Command::new("launchctl")
        .args(["list", label])
        .output()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if output.status.success() && !stdout.trim().is_empty() {
        print!("{stdout}");
    } else {
        println!("{label} is not loaded.");
        println!("Run: mynd service install");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_install_macos(dashboard: bool, matrix: bool) -> Result<()> {
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "Mynd server (API + dashboard)")
    } else {
        (&["up", "--headless"], "Mynd server (API only)")
    };
    service_install_unit_macos(LAUNCH_AGENT_LABEL, args, desc)?;

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            anyhow::bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `mynd matrix login` first, then re-run `mynd service install --matrix`."
            );
        }
        service_install_unit_macos(
            MATRIX_LAUNCH_AGENT_LABEL,
            &["matrix", "run"],
            "Mynd Matrix chat bot",
        )?;
    }

    println!();
    println!("Mynd will now start automatically on login.");
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        println!("Dashboard: http://127.0.0.1:{port}");
    }
    println!("Logs: ~/Library/Logs/hivemind.log");
    println!("Check status: mynd service status");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_uninstall_macos() -> Result<()> {
    service_uninstall_unit_macos(LAUNCH_AGENT_LABEL)?;
    if launch_agent_path(MATRIX_LAUNCH_AGENT_LABEL).exists() {
        service_uninstall_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?;
    }

    println!("Mynd service uninstalled.");
    Ok(())
}

#[cfg(target_os = "macos")]
fn service_status_macos() -> Result<()> {
    service_status_unit_macos(LAUNCH_AGENT_LABEL)?;
    if launch_agent_path(MATRIX_LAUNCH_AGENT_LABEL).exists() {
        service_status_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?;
    }
    Ok(())
}
