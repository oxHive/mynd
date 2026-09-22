use super::init::home_dir;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

// ── service management ────────────────────────────────────────────────────────
// Unit / agent names carry the `mynd` name (launchd labels use reverse-DNS of
// oxhive.dev). Older names are kept only so `install` / `uninstall` can tear
// down units left behind by a previous build (see remove_legacy_units_* below).
//
// Structured the same way as wardn's `service` module: each unit-level
// operation is a pure function returning `Result<String>` — a hard error on
// a real failure (`run_ok`), a composed human-readable message on success —
// and the `cmd_service_*` entry points below print the composed result once,
// instead of the individual steps printing (or warning) as they go.

#[cfg(target_os = "linux")]
const CURRENT_UNIT: &str = "mynd";
#[cfg(target_os = "linux")]
const CURRENT_MATRIX_UNIT: &str = "mynd-matrix";
#[cfg(target_os = "linux")]
const CURRENT_DISCORD_UNIT: &str = "mynd-discord";
#[cfg(target_os = "linux")]
const LEGACY_UNITS: [&str; 2] = ["hivemind", "hivemind-matrix"];

pub fn cmd_service_install(
    dashboard: bool,
    matrix: bool,
    hive: bool,
    discord: bool,
    no_linger: bool,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        let _ = no_linger; // launchd has no linger equivalent — see service_install_macos.
        println!(
            "{}",
            service_install_macos(dashboard, matrix, hive, discord)?
        );
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        println!(
            "{}",
            service_install_linux(dashboard, matrix, hive, discord, !no_linger)?
        );
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = (dashboard, matrix, hive, discord, no_linger);
        bail!("mynd service install is only supported on Linux and macOS");
    }
}

pub fn cmd_service_uninstall() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        println!("{}", service_uninstall_macos()?);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        println!("{}", service_uninstall_linux()?);
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    bail!("mynd service uninstall is only supported on Linux and macOS");
}

pub fn cmd_service_status() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        println!("{}", service_status_macos()?);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        println!("{}", service_status_linux()?);
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    bail!("mynd service status is only supported on Linux and macOS");
}

// ── shared helpers ──────────────────────────────────────────────────────────

/// Runs `cmd`, hard-failing with its stderr on a non-zero exit — ported from
/// wardn's `service::run_ok` so a failed `systemctl`/`launchctl` call aborts
/// the command instead of printing a warning and silently continuing.
#[allow(dead_code)]
fn run_ok(cmd: &mut Command) -> Result<()> {
    let program = format!("{cmd:?}");
    let output = cmd.output().with_context(|| format!("running {program}"))?;
    if !output.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(())
}

#[allow(dead_code)]
fn command_stdout(cmd: &mut Command) -> Option<String> {
    let text = cmd
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())?;
    (!text.is_empty()).then_some(text)
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
) -> Result<String> {
    let exe = std::env::current_exe().context("locating the mynd binary")?;
    let unit = systemd_unit_content(description, &exe, exec_args);

    let unit_path = systemd_unit_path(unit_name);
    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&unit_path, &unit)
        .with_context(|| format!("writing {}", unit_path.display()))?;

    run_ok(Command::new("systemctl").args(["--user", "daemon-reload"]))?;
    run_ok(Command::new("systemctl").args(["--user", "enable", "--now", unit_name]))?;
    // `enable --now` only starts the unit if it wasn't already running, so a
    // reinstall (e.g. after `--dashboard` changed) needs an explicit restart
    // to pick up the rewritten ExecStart/Environment lines.
    run_ok(Command::new("systemctl").args(["--user", "restart", unit_name]))?;

    Ok(format!(
        "Installed {} and started it (enabled to run on login).",
        unit_path.display()
    ))
}

#[cfg(target_os = "linux")]
fn service_uninstall_unit_linux(unit_name: &str) -> Result<String> {
    let unit_path = systemd_unit_path(unit_name);
    if !unit_path.exists() {
        return Ok(format!(
            "{} is not installed — nothing to do",
            unit_path.display()
        ));
    }
    // Best-effort: an already-stopped or half-broken unit shouldn't block
    // removing its file.
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", unit_name])
        .output();
    std::fs::remove_file(&unit_path)
        .with_context(|| format!("removing {}", unit_path.display()))?;
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .output();
    Ok(format!("Stopped and removed {}", unit_path.display()))
}

#[cfg(target_os = "linux")]
fn service_status_unit_linux(unit_name: &str) -> Result<String> {
    let unit_path = systemd_unit_path(unit_name);
    if !unit_path.exists() {
        return Ok(format!(
            "{unit_name}: not installed (expected {})",
            unit_path.display()
        ));
    }
    let enabled =
        command_stdout(Command::new("systemctl").args(["--user", "is-enabled", unit_name]));
    let active = command_stdout(Command::new("systemctl").args(["--user", "is-active", unit_name]));
    Ok(format!(
        "{unit_name}: installed ({}) — enabled={} active={}",
        unit_path.display(),
        enabled.as_deref().unwrap_or("unknown"),
        active.as_deref().unwrap_or("unknown"),
    ))
}

/// Best-effort teardown of units written by a pre-rename (`hivemind`) build, so
/// an upgraded machine does not end up with two competing services.
#[cfg(target_os = "linux")]
fn remove_legacy_units_linux() {
    for unit in LEGACY_UNITS {
        if systemd_unit_path(unit).exists() {
            let _ = service_uninstall_unit_linux(unit);
        }
    }
}

/// Whether `loginctl enable-linger` ran, and how it went — mirrors wardn's
/// `service::LingerOutcome`, kept the same shape across both products since
/// the underlying systemd behavior (and the message a user reads) is
/// identical.
#[cfg(target_os = "linux")]
enum LingerOutcome {
    Skipped,
    Enabled,
    Failed(String),
}

/// `WantedBy=default.target` alone only starts a systemd --user unit when
/// this user logs in — a `systemd --user` manager doesn't run at boot
/// unless lingering is on. Without it, a headless box that reboots stays
/// down until someone logs in again.
#[cfg(target_os = "linux")]
fn enable_linger_now() -> LingerOutcome {
    match std::process::Command::new("loginctl")
        .arg("enable-linger")
        .output()
    {
        Ok(out) if out.status.success() => LingerOutcome::Enabled,
        Ok(out) => LingerOutcome::Failed(String::from_utf8_lossy(&out.stderr).trim().to_string()),
        Err(e) => LingerOutcome::Failed(e.to_string()),
    }
}

#[cfg(target_os = "linux")]
fn linger_note(outcome: &LingerOutcome) -> String {
    match outcome {
        LingerOutcome::Enabled => {
            "Linger enabled — this also starts the service at boot, without needing a login."
                .to_string()
        }
        LingerOutcome::Skipped => {
            "Linger not enabled (--no-linger) — the service starts on login, not at boot. \
             Enable it later with `loginctl enable-linger $USER`."
                .to_string()
        }
        LingerOutcome::Failed(err) => format!(
            "Could not enable linger automatically ({err}) — the service still starts on \
             login, but not at boot until you run `loginctl enable-linger $USER` yourself."
        ),
    }
}

#[cfg(target_os = "linux")]
fn linger_status_line() -> String {
    let username = std::process::Command::new("whoami")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let linger = username.and_then(|user| {
        std::process::Command::new("loginctl")
            .args(["show-user", "--value", "-p", "Linger", &user])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
    });
    match linger.as_deref() {
        Some("yes") => {
            "linger: enabled — this service also starts at boot, without needing a login"
                .to_string()
        }
        Some("no") => "linger: disabled — starts on login only (`loginctl enable-linger $USER` \
                        to also start at boot)"
            .to_string(),
        _ => "linger: unknown".to_string(),
    }
}

#[cfg(target_os = "linux")]
fn service_install_linux(
    dashboard: bool,
    matrix: bool,
    hive: bool,
    discord: bool,
    enable_linger: bool,
) -> Result<String> {
    remove_legacy_units_linux();
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "Mynd server (API + dashboard)")
    } else {
        (&["up", "--headless"], "Mynd server (API only)")
    };
    let mut messages = vec![service_install_unit_linux(CURRENT_UNIT, desc, args)?];

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `mynd matrix login` first, then re-run `mynd service install --matrix`."
            );
        }
        messages.push(service_install_unit_linux(
            CURRENT_MATRIX_UNIT,
            "Mynd Matrix chat bot",
            &["matrix", "run"],
        )?);
    }

    if hive {
        let configured = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.hive.enabled)
            .unwrap_or(false);
        if !configured {
            bail!(
                "--hive was passed but Hive Mode is not enabled.\n\
                 Set [hive] enabled = true in ~/.config/mynd/config.toml first, \
                 then re-run `mynd service install --hive`."
            );
        }
        // Hive sync runs inside the main `mynd up` process (unlike the Matrix
        // bot, which is a wholly separate daemon) -- no separate systemd unit to
        // install here. This block exists purely as the same fail-fast precondition
        // check the --matrix flag already does, for consistency.
    }

    if discord {
        let configured = crate::config::load_discord_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            bail!(
                "--discord was passed but Discord is not configured.\n\
                 Run `mynd discord login` first, then re-run `mynd service install --discord`."
            );
        }
        messages.push(service_install_unit_linux(
            CURRENT_DISCORD_UNIT,
            "Mynd Discord chat bot",
            &["discord", "run"],
        )?);
    }

    let linger = if enable_linger {
        enable_linger_now()
    } else {
        LingerOutcome::Skipped
    };

    messages.push(String::new());
    messages.push("Mynd will now start automatically on login.".to_string());
    messages.push(linger_note(&linger));
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        messages.push(format!("Dashboard: http://127.0.0.1:{port}"));
    }
    messages.push("Check on it any time with `mynd service status`.".to_string());

    Ok(messages.join("\n"))
}

#[cfg(target_os = "linux")]
fn service_uninstall_linux() -> Result<String> {
    remove_legacy_units_linux();
    let messages = [
        service_uninstall_unit_linux(CURRENT_UNIT)?,
        service_uninstall_unit_linux(CURRENT_MATRIX_UNIT)?,
        service_uninstall_unit_linux(CURRENT_DISCORD_UNIT)?,
    ];
    Ok(messages.join("\n"))
}

#[cfg(target_os = "linux")]
fn service_status_linux() -> Result<String> {
    let mut messages = vec![
        service_status_unit_linux(CURRENT_UNIT)?,
        service_status_unit_linux(CURRENT_MATRIX_UNIT)?,
        service_status_unit_linux(CURRENT_DISCORD_UNIT)?,
    ];
    messages.push(linger_status_line());
    Ok(messages.join("\n"))
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
    fn systemd_unit_content_for_discord_names_the_unit_and_subcommand() {
        let content = systemd_unit_content(
            "Mynd Discord chat bot",
            &std::path::PathBuf::from("/usr/local/bin/mynd"),
            &["discord", "run"],
        );
        assert!(content.contains("Description=Mynd Discord chat bot"));
        assert!(content.contains("ExecStart=/usr/local/bin/mynd discord run"));
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

    #[test]
    fn linger_note_enabled_mentions_boot() {
        assert!(linger_note(&LingerOutcome::Enabled).contains("boot"));
    }

    #[test]
    fn linger_note_skipped_explains_how_to_enable_it_later() {
        let note = linger_note(&LingerOutcome::Skipped);
        assert!(note.contains("--no-linger"));
        assert!(note.contains("loginctl enable-linger $USER"));
    }

    #[test]
    fn linger_note_failed_surfaces_the_underlying_error() {
        let note = linger_note(&LingerOutcome::Failed(
            "Interactive authentication required.".into(),
        ));
        assert!(note.contains("Interactive authentication required."));
        assert!(note.contains("loginctl enable-linger $USER"));
    }

    #[test]
    fn service_uninstall_unit_linux_reports_when_nothing_is_installed() {
        // systemd_unit_path resolves under XDG_CONFIG_HOME/systemd/user, so
        // pointing it at an empty temp dir guarantees "not installed" without
        // touching the real machine's systemd user config.
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let message = service_uninstall_unit_linux("definitely-not-a-real-mynd-unit").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert!(message.contains("is not installed — nothing to do"));
    }

    #[test]
    fn service_status_unit_linux_reports_when_nothing_is_installed() {
        let dir = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        let message = service_status_unit_linux("definitely-not-a-real-mynd-unit").unwrap();
        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        assert!(message.contains("not installed (expected"));
    }

    #[test]
    fn unit_names_carry_the_mynd_name_and_legacy_names_are_the_hivemind_ones() {
        assert_eq!(CURRENT_UNIT, "mynd");
        assert_eq!(CURRENT_MATRIX_UNIT, "mynd-matrix");
        assert_eq!(CURRENT_DISCORD_UNIT, "mynd-discord");
        assert!(
            systemd_unit_path(CURRENT_UNIT).ends_with("mynd.service"),
            "unit path: {}",
            systemd_unit_path(CURRENT_UNIT).display()
        );
        assert_eq!(LEGACY_UNITS, ["hivemind", "hivemind-matrix"]);
    }
}

// ── macOS / launchd ───────────────────────────────────────────────────────────

// Reverse-DNS of the domain oxHive controls (oxhive.dev).
#[cfg(target_os = "macos")]
const LAUNCH_AGENT_LABEL: &str = "dev.oxhive.mynd";

#[cfg(target_os = "macos")]
const MATRIX_LAUNCH_AGENT_LABEL: &str = "dev.oxhive.mynd-matrix";

/// Labels written by older builds, torn down on install/uninstall so an
/// upgraded machine does not keep an orphaned LaunchAgent loaded. Covers the
/// pre-rename name and the earlier `com.oxhive.*` prefix.
#[cfg(target_os = "macos")]
const LEGACY_LAUNCH_AGENT_LABELS: [&str; 4] = [
    "com.oxhive.hivemind",
    "com.oxhive.hivemind-matrix",
    "com.oxhive.mynd",
    "com.oxhive.mynd-matrix",
];

#[cfg(target_os = "macos")]
const DISCORD_LAUNCH_AGENT_LABEL: &str = "dev.oxhive.mynd-discord";

#[cfg(target_os = "macos")]
fn launch_agent_path(label: &str) -> PathBuf {
    home_dir()
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{label}.plist"))
}

#[cfg(target_os = "macos")]
fn launch_agent_plist_content(
    label: &str,
    exe: &Path,
    exec_args: &[&str],
    path_env: &str,
) -> String {
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
           <key>EnvironmentVariables</key>\n\
           <dict>\n\
             <key>PATH</key>\n\
             <string>{path_env}</string>\n\
           </dict>\n\
           <key>RunAtLoad</key>\n\
           <true/>\n\
           <key>KeepAlive</key>\n\
           <true/>\n\
           <key>StandardOutPath</key>\n\
           <string>{log_dir}/mynd.log</string>\n\
           <key>StandardErrorPath</key>\n\
           <string>{log_dir}/mynd.log</string>\n\
         </dict>\n\
         </plist>\n",
        log_dir = log_dir.display(),
    )
}

/// Unlike a systemd --user unit, a LaunchAgent has no lingering equivalent:
/// it only ever starts when this user's launchd session starts, which
/// happens on login, never unattended at boot — matches wardn's launchd
/// install, which skips the linger flag entirely for the same reason.
#[cfg(target_os = "macos")]
fn service_install_unit_macos(
    label: &str,
    exec_args: &[&str],
    description: &str,
) -> Result<String> {
    let exe = std::env::current_exe().context("locating the mynd binary")?;
    let plist_path = launch_agent_path(label);
    // See the systemd path_env comment in service_install_unit_linux: launchd
    // agents get the same minimal-PATH problem, so bake in the PATH from this
    // (interactive) invocation.
    let path_env =
        std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".to_string());
    let plist = launch_agent_plist_content(label, &exe, exec_args, &path_env);

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(&plist_path, &plist)
        .with_context(|| format!("writing {}", plist_path.display()))?;

    // Best-effort: unload any stale copy (e.g. from a previous install)
    // before loading the fresh one, so a reinstall actually picks up
    // changed args instead of `launchctl load` no-op'ing against an
    // already-loaded label.
    let _ = Command::new("launchctl")
        .args(["unload", "-w", plist_path.to_str().unwrap()])
        .output();
    run_ok(Command::new("launchctl").args(["load", "-w", plist_path.to_str().unwrap()]))?;

    Ok(format!(
        "Installed {} ({description}) and started it (enabled to run on login).",
        plist_path.display()
    ))
}

#[cfg(target_os = "macos")]
fn service_uninstall_unit_macos(label: &str) -> Result<String> {
    let plist_path = launch_agent_path(label);
    if !plist_path.exists() {
        return Ok(format!(
            "{} is not installed — nothing to do",
            plist_path.display()
        ));
    }
    let _ = Command::new("launchctl")
        .args(["unload", "-w", plist_path.to_str().unwrap()])
        .output();
    std::fs::remove_file(&plist_path)
        .with_context(|| format!("removing {}", plist_path.display()))?;
    Ok(format!("Stopped and removed {}", plist_path.display()))
}

#[cfg(target_os = "macos")]
fn service_status_unit_macos(label: &str) -> Result<String> {
    let plist_path = launch_agent_path(label);
    if !plist_path.exists() {
        return Ok(format!(
            "{label}: not installed (expected {})",
            plist_path.display()
        ));
    }
    match Command::new("launchctl").args(["list", label]).output() {
        Ok(out) if out.status.success() => {
            let text = String::from_utf8_lossy(&out.stdout);
            Ok(format!(
                "{label}: installed ({}) — loaded\n{}",
                plist_path.display(),
                text.trim()
            ))
        }
        _ => Ok(format!(
            "{label}: installed ({}) but not loaded — run `mynd service install` again",
            plist_path.display()
        )),
    }
}

/// Best-effort teardown of LaunchAgents written by a pre-rename (`hivemind`)
/// build, so an upgraded machine does not run two competing agents.
#[cfg(target_os = "macos")]
fn remove_legacy_units_macos() {
    for label in LEGACY_LAUNCH_AGENT_LABELS {
        if launch_agent_path(label).exists() {
            let _ = service_uninstall_unit_macos(label);
        }
    }
}

#[cfg(target_os = "macos")]
fn service_install_macos(
    dashboard: bool,
    matrix: bool,
    hive: bool,
    discord: bool,
) -> Result<String> {
    remove_legacy_units_macos();
    let (args, desc): (&[&str], &str) = if dashboard {
        (&["up"], "Mynd server (API + dashboard)")
    } else {
        (&["up", "--headless"], "Mynd server (API only)")
    };
    let mut messages = vec![service_install_unit_macos(LAUNCH_AGENT_LABEL, args, desc)?];

    if matrix {
        let configured = crate::config::load_matrix_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            bail!(
                "--matrix was passed but Matrix is not configured.\n\
                 Run `mynd matrix login` first, then re-run `mynd service install --matrix`."
            );
        }
        messages.push(service_install_unit_macos(
            MATRIX_LAUNCH_AGENT_LABEL,
            &["matrix", "run"],
            "Mynd Matrix chat bot",
        )?);
    }

    if hive {
        let configured = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.hive.enabled)
            .unwrap_or(false);
        if !configured {
            bail!(
                "--hive was passed but Hive Mode is not enabled.\n\
                 Set [hive] enabled = true in ~/.config/mynd/config.toml first, \
                 then re-run `mynd service install --hive`."
            );
        }
        // Hive sync runs inside the main `mynd up` process (unlike the Matrix
        // bot, which is a wholly separate daemon) -- no separate systemd unit to
        // install here. This block exists purely as the same fail-fast precondition
        // check the --matrix flag already does, for consistency.
    }

    if discord {
        let configured = crate::config::load_discord_settings(&crate::config::global_config_path())
            .ok()
            .flatten()
            .is_some();
        if !configured {
            bail!(
                "--discord was passed but Discord is not configured.\n\
                 Run `mynd discord login` first, then re-run `mynd service install --discord`."
            );
        }
        messages.push(service_install_unit_macos(
            DISCORD_LAUNCH_AGENT_LABEL,
            &["discord", "run"],
            "Mynd Discord chat bot",
        )?);
    }

    messages.push(String::new());
    messages.push("Mynd will now start automatically on login.".to_string());
    messages.push(
        "This starts again on login, not unattended at boot — see auto-login if this needs \
         to survive a reboot with nobody signed in."
            .to_string(),
    );
    if dashboard {
        let port = crate::config::load_server_settings(&crate::config::global_config_path())
            .map(|s| s.dashboard_port)
            .unwrap_or(3457);
        messages.push(format!("Dashboard: http://127.0.0.1:{port}"));
    }
    messages.push("Logs: ~/Library/Logs/mynd.log".to_string());
    messages.push("Check on it any time with `mynd service status`.".to_string());

    Ok(messages.join("\n"))
}

#[cfg(target_os = "macos")]
fn service_uninstall_macos() -> Result<String> {
    remove_legacy_units_macos();
    let messages = [
        service_uninstall_unit_macos(LAUNCH_AGENT_LABEL)?,
        service_uninstall_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?,
        service_uninstall_unit_macos(DISCORD_LAUNCH_AGENT_LABEL)?,
    ];
    Ok(messages.join("\n"))
}

#[cfg(target_os = "macos")]
fn service_status_macos() -> Result<String> {
    let messages = [
        service_status_unit_macos(LAUNCH_AGENT_LABEL)?,
        service_status_unit_macos(MATRIX_LAUNCH_AGENT_LABEL)?,
        service_status_unit_macos(DISCORD_LAUNCH_AGENT_LABEL)?,
    ];
    Ok(messages.join("\n"))
}
