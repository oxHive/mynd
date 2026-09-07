use crate::cli::{DiscordStatusLine, MatrixStatusLine, StatusData, build_status_data};
use crate::store::SqliteStore;
use crate::tui::{TerminalGuard, header::render_header};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Padding, Paragraph},
};
use std::path::Path;
use std::time::Duration;

const REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const DIM: Color = Color::Rgb(0x8a, 0x8a, 0x9a);
const WARNING: Color = Color::Rgb(0xf5, 0xa5, 0x24);
/// Inline viewport height in rows: renders as a compact panel under the
/// shell prompt rather than taking over the full screen.
const VIEWPORT_HEIGHT: u16 = 12;

/// Runs the interactive `mynd status` view: header + a key-value panel
/// that auto-refreshes every 5s, with `r` for an immediate manual refresh.
/// `q` / Ctrl+C exits. Returns once the user quits.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    cwd: &Path,
    global_path: &Path,
    store: &SqliteStore,
    db_path: &str,
    registered_clients: &[&str],
    settings: &crate::config::ServerSettings,
    server_up: bool,
) -> Result<()> {
    let guard = TerminalGuard::enter(VIEWPORT_HEIGHT)?;
    let mut terminal = guard.terminal()?;

    let mut data = build_status_data(
        cwd,
        global_path,
        store,
        db_path,
        registered_clients,
        settings,
        server_up,
    )
    .await?;

    let mut ticker = tokio::time::interval(REFRESH_INTERVAL);
    ticker.tick().await; // first tick fires immediately; consume it, we already fetched above
    let no_color = crate::tui::no_color();
    let mut last_error: Option<String> = None;
    let mut last_message: Option<String> = None;

    // Refreshes `data` in place, re-probing whether the server is up rather
    // than trusting the possibly-stale `server_up` this fn was first called
    // with — otherwise a server started/killed while this view is open would
    // never be reflected. On success, clears `last_error`; on failure, leaves
    // `data` untouched (stale but valid) and records the error so it can be
    // shown inline. Never aborts the loop over a failed refresh.
    async fn refresh(
        cwd: &Path,
        global_path: &Path,
        store: &SqliteStore,
        db_path: &str,
        registered_clients: &[&str],
        settings: &crate::config::ServerSettings,
        data: &mut StatusData,
        last_error: &mut Option<String>,
    ) {
        let probe_settings = settings.clone();
        let server_up =
            tokio::task::spawn_blocking(move || crate::cli::probe_server_up(&probe_settings))
                .await
                .unwrap_or(false);
        match build_status_data(
            cwd,
            global_path,
            store,
            db_path,
            registered_clients,
            settings,
            server_up,
        )
        .await
        {
            Ok(new_data) => {
                *data = new_data;
                *last_error = None;
            }
            Err(e) => {
                *last_error = Some(e.to_string());
            }
        }
    }

    loop {
        terminal.draw(|frame| {
            draw(
                &data,
                last_error.as_deref(),
                last_message.as_deref(),
                no_color,
                frame,
            )
        })?;

        tokio::select! {
            _ = ticker.tick() => {
                refresh(
                    cwd, global_path, store, db_path, registered_clients, settings,
                    &mut data, &mut last_error,
                )
                .await;
            }
            key = poll_key_event() => {
                if let Some(key) = key {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                        KeyCode::Char('r') => {
                            last_message = None;
                            refresh(
                                cwd, global_path, store, db_path, registered_clients, settings,
                                &mut data, &mut last_error,
                            )
                            .await;
                        }
                        KeyCode::Char('k') if data.server_up => {
                            last_message = Some(tokio::task::spawn_blocking(kill_server).await.unwrap_or_else(|_| "kill task panicked".to_string()));
                            refresh(
                                cwd, global_path, store, db_path, registered_clients, settings,
                                &mut data, &mut last_error,
                            )
                            .await;
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
}

/// Sends SIGTERM to the PID recorded in `mynd up`'s pidfile and waits
/// briefly for it to exit. Shells out to `kill` rather than adding a signal
/// crate dependency — matches the project's existing Unix-only assumptions
/// (XDG paths, $HOME, the `open`/`xdg-open` dashboard launcher). Always
/// leaves the pidfile removed, even on a stale or unresponsive PID, so a
/// dead server never blocks a later `k` press.
fn kill_server() -> String {
    let path = crate::db::up_pidfile_path();
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return "no server pidfile found".to_string();
    };
    let Ok(pid) = contents.trim().parse::<u32>() else {
        let _ = std::fs::remove_file(&path);
        return "pidfile was corrupt; removed it".to_string();
    };
    if !process_alive(pid) {
        let _ = std::fs::remove_file(&path);
        return "server was already stopped; removed stale pidfile".to_string();
    }
    let sent = std::process::Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !sent {
        return format!("failed to signal pid {pid}");
    }
    for _ in 0..20 {
        if !process_alive(pid) {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::fs::remove_file(&path);
    if process_alive(pid) {
        format!("sent SIGTERM to pid {pid}, still shutting down")
    } else {
        format!("stopped server (pid {pid})")
    }
}

/// `kill -0` (and `-TERM` above) inherit stdio by default, so an unsilenced
/// call prints things like "kill: sending signal to 123 failed: No such
/// process" straight into the raw-mode terminal — corrupting the TUI frame
/// mid-render, since nothing here goes through ratatui's buffer. Silenced;
/// the exit status alone tells us what we need.
fn process_alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Polls for a key-press event on a blocking thread (crossterm's `poll`/`read`
/// are synchronous) without blocking the tokio runtime. Returns `None` on a
/// 100ms timeout with no event, or a non-press event (e.g. key release).
async fn poll_key_event() -> Option<event::KeyEvent> {
    tokio::task::spawn_blocking(|| {
        if event::poll(Duration::from_millis(100)).unwrap_or(false)
            && let Ok(Event::Key(key)) = event::read()
            && key.kind == KeyEventKind::Press
        {
            return Some(key);
        }
        None
    })
    .await
    .unwrap_or(None)
}

/// Border (2) + left/right padding (2+2) added around the body's content
/// width to get the box's total column width.
const BODY_FRAME_OVERHEAD: u16 = 6;
/// Floor so the box never shrinks below fitting the header wordmark/title.
const MIN_BOX_WIDTH: u16 = 40;
fn draw(
    data: &StatusData,
    last_error: Option<&str>,
    last_message: Option<&str>,
    no_color: bool,
    frame: &mut ratatui::Frame,
) {
    let footer_text = if data.server_up {
        "  q quit   r refresh   k kill server"
    } else {
        "  q quit   r refresh"
    };
    let mut lines = vec![
        Line::from(format!(
            "Server     {}",
            if data.server_up {
                format!(
                    "running at http://{}:{}",
                    data.server_host, data.server_port
                )
            } else {
                "not running".to_string()
            }
        )),
        Line::from(format!("Storage    {}", data.db_path)),
        Line::from(format!(
            "Sync       {}",
            if data.sync_enabled {
                format!("enabled -> {}", data.sync_remote_url)
            } else {
                "disabled (local only)".to_string()
            }
        )),
        Line::from(format!(
            "AI clients {}",
            if data.registered_clients.is_empty() {
                "none registered".to_string()
            } else {
                data.registered_clients.join(", ")
            }
        )),
    ];
    match &data.matrix {
        MatrixStatusLine::NotConfigured => {}
        MatrixStatusLine::NotRunning => {
            lines.push(Line::from("Matrix     configured, not running"));
        }
        MatrixStatusLine::Running {
            user_id,
            sync_state,
            room_count,
            active_sessions,
        } => {
            lines.push(Line::from(format!(
                "Matrix     {user_id} ({sync_state}), {room_count} room(s), \
                 {active_sessions} active session(s)"
            )));
        }
    }
    match &data.discord {
        DiscordStatusLine::NotConfigured => {}
        DiscordStatusLine::NotRunning => {
            lines.push(Line::from("Discord    configured, not running"));
        }
        DiscordStatusLine::Running {
            application_id,
            sync_state,
            channel_count,
            active_sessions,
        } => {
            lines.push(Line::from(format!(
                "Discord    {application_id} ({sync_state}), {channel_count} channel(s), \
                 {active_sessions} active session(s)"
            )));
        }
    }
    if data.project.is_none() {
        lines.push(Line::from(""));
        lines.push(Line::from("No .mynd.toml found in this directory tree."));
    }

    // Error takes priority over an info message when both are somehow set;
    // only one notice row is budgeted below.
    let notice = last_error
        .map(|m| (format!("refresh failed: {m}"), WARNING))
        .or_else(|| last_message.map(|m| (m.to_string(), DIM)));

    // Size the box to the widest line instead of a fixed width, so long
    // values (storage path, sync URL) fit without wrapping or clipping.
    let content_width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(footer_text.len()) as u16;

    let area = frame.area();
    let width = (content_width + BODY_FRAME_OVERHEAD)
        .max(MIN_BOX_WIDTH)
        .min(area.width);
    let area = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).split(area)[0];
    let notice_height = if notice.is_some() { 1 } else { 0 };
    let layout = Layout::vertical([
        Constraint::Length(5),
        Constraint::Length(notice_height),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(data, no_color, layout[0], frame.buffer_mut());

    if let Some((msg, color)) = notice {
        let notice_style = if no_color {
            Style::default()
        } else {
            Style::default().fg(color)
        };
        frame.render_widget(
            Paragraph::new(Line::from(msg).style(notice_style)),
            layout[1],
        );
    }

    let body = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::new(2, 2, 0, 0))
        .title(" Overview ");
    let inner = body.inner(layout[2]);
    frame.render_widget(body, layout[2]);
    frame.render_widget(Paragraph::new(lines), inner);

    let footer_style = if no_color {
        Style::default()
    } else {
        Style::default().fg(DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(footer_text).style(footer_style)),
        layout[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::ProjectStatus;
    use ratatui::{Terminal, backend::TestBackend};

    fn sample_data() -> StatusData {
        StatusData {
            version: "0.14.3",
            project_label: None,
            server_up: true,
            server_host: "127.0.0.1".to_string(),
            server_port: 3456,
            db_path: "~/.local/share/hivemind/memories.db".to_string(),
            memory_count: 42,
            sync_enabled: false,
            sync_remote_url: String::new(),
            registered_clients: vec![],
            project: None,
            matrix: MatrixStatusLine::NotConfigured,
            discord: DiscordStatusLine::NotConfigured,
        }
    }

    fn render(data: &StatusData, last_error: Option<&str>, last_message: Option<&str>) -> String {
        render_no_color(data, last_error, last_message, false)
    }

    fn render_no_color(
        data: &StatusData,
        last_error: Option<&str>,
        last_message: Option<&str>,
        no_color: bool,
    ) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(data, last_error, last_message, no_color, frame))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn draw_shows_server_running_and_storage_path() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(content.contains("running at http://127.0.0.1:3456"));
        assert!(content.contains("~/.local/share/hivemind/memories.db"));
    }

    #[test]
    fn draw_shows_server_not_running() {
        let mut data = sample_data();
        data.server_up = false;
        let content = render(&data, None, None);
        assert!(content.contains("not running"));
    }

    #[test]
    fn draw_shows_sync_enabled_and_url() {
        let mut data = sample_data();
        data.sync_enabled = true;
        data.sync_remote_url = "https://sync.example.com".to_string();
        let content = render(&data, None, None);
        assert!(content.contains("enabled"));
        assert!(content.contains("https://sync.example.com"));
    }

    #[test]
    fn draw_shows_sync_disabled_by_default() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(content.contains("disabled"));
    }

    #[test]
    fn draw_shows_registered_clients_when_present() {
        let mut data = sample_data();
        data.registered_clients = vec!["claude".to_string(), "opencode".to_string()];
        let content = render(&data, None, None);
        assert!(content.contains("claude"));
        assert!(content.contains("opencode"));
    }

    #[test]
    fn draw_shows_none_registered_when_empty() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(content.contains("none registered"));
    }

    #[test]
    fn draw_omits_matrix_and_discord_lines_when_not_configured() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(!content.contains("Matrix"));
        assert!(!content.contains("Discord"));
    }

    #[test]
    fn draw_shows_matrix_not_running() {
        let mut data = sample_data();
        data.matrix = MatrixStatusLine::NotRunning;
        let content = render(&data, None, None);
        assert!(content.contains("Matrix"));
        assert!(content.contains("configured, not running"));
    }

    #[test]
    fn draw_shows_matrix_running_details() {
        let mut data = sample_data();
        data.matrix = MatrixStatusLine::Running {
            user_id: "@bot:matrix.org".to_string(),
            sync_state: "synced".to_string(),
            room_count: 3,
            active_sessions: 1,
        };
        let content = render(&data, None, None);
        assert!(content.contains("@bot:matrix.org"));
        assert!(content.contains("synced"));
        assert!(content.contains("3 room(s)"));
        assert!(content.contains("1 active session(s)"));
    }

    #[test]
    fn draw_shows_discord_not_running() {
        let mut data = sample_data();
        data.discord = DiscordStatusLine::NotRunning;
        let content = render(&data, None, None);
        assert!(content.contains("Discord"));
        assert!(content.contains("configured, not running"));
    }

    #[test]
    fn draw_shows_discord_running_details() {
        let mut data = sample_data();
        data.discord = DiscordStatusLine::Running {
            application_id: "123456789012345678".to_string(),
            sync_state: "connected".to_string(),
            channel_count: 2,
            active_sessions: 1,
        };
        let content = render(&data, None, None);
        assert!(content.contains("123456789012345678"));
        assert!(content.contains("connected"));
        assert!(content.contains("2 channel(s)"));
    }

    #[test]
    fn draw_shows_no_project_message_when_project_is_none() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(content.contains("No .mynd.toml found in this directory tree."));
    }

    #[test]
    fn draw_omits_no_project_message_when_project_is_some() {
        let mut data = sample_data();
        data.project = Some(ProjectStatus {
            project_name: "hivemind".to_string(),
            has_local_config: false,
            file_open_rule_count: 0,
            mention_trigger_count: 0,
            loaded: vec![],
            skipped: vec![],
            used_tokens: 0,
            max_tokens: 1000,
            truncated: false,
        });
        let content = render(&data, None, None);
        assert!(!content.contains("No .mynd.toml found in this directory tree."));
    }

    #[test]
    fn draw_shows_error_notice_when_last_error_set() {
        let data = sample_data();
        let content = render(&data, Some("connection refused"), None);
        assert!(content.contains("refresh failed: connection refused"));
    }

    #[test]
    fn draw_shows_info_message_when_last_message_set_and_no_error() {
        let data = sample_data();
        let content = render(&data, None, Some("stopped server (pid 123)"));
        assert!(content.contains("stopped server (pid 123)"));
    }

    #[test]
    fn draw_error_takes_priority_over_last_message() {
        let data = sample_data();
        let content = render(&data, Some("boom"), Some("should not appear"));
        assert!(content.contains("refresh failed: boom"));
        assert!(!content.contains("should not appear"));
    }

    #[test]
    fn draw_shows_kill_shortcut_when_server_up() {
        let data = sample_data();
        let content = render(&data, None, None);
        assert!(content.contains("k kill server"));
    }

    #[test]
    fn draw_omits_kill_shortcut_when_server_down() {
        let mut data = sample_data();
        data.server_up = false;
        let content = render(&data, None, None);
        assert!(!content.contains("k kill server"));
        assert!(content.contains("q quit"));
    }

    #[test]
    fn draw_no_color_skips_foreground_styling_but_keeps_text() {
        let data = sample_data();
        let content = render_no_color(&data, Some("boom"), None, true);
        assert!(content.contains("refresh failed: boom"));

        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(&data, Some("boom"), None, true, frame))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        for cell in buffer.content.iter() {
            assert_eq!(
                cell.fg,
                Color::Reset,
                "cell {:?} should have no foreground color set when no_color=true",
                cell.symbol()
            );
        }
    }
}
