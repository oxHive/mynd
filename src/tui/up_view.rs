use crate::cli::StatusData;
use crate::store::SqliteStore;
use crate::tui::{TerminalGuard, header::render_header};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph},
};
use std::collections::VecDeque;
use std::time::Duration;
use tokio::sync::broadcast;

const MAX_FEED_LINES: usize = 200;
const DIM: Color = Color::Rgb(0x8a, 0x8a, 0x9a);
const CYAN: Color = Color::Rgb(0x67, 0xe8, 0xf9);
/// Inline viewport height in rows: renders as a compact panel under the
/// shell prompt rather than taking over the full screen.
const VIEWPORT_HEIGHT: u16 = 12;
/// Border (2) + left/right padding (2+2) added around the body's content
/// width to get the box's total column width.
const BODY_FRAME_OVERHEAD: u16 = 6;
/// Floor so the box never shrinks below fitting the header wordmark/title.
const MIN_BOX_WIDTH: u16 = 40;
const FOOTER_TEXT: &str = "  d detach   ctrl+c stop server";

/// Runs the interactive `mynd up` view: header + a live activity feed fed
/// by the existing SSE broadcast channel. Returns on `d` — the caller
/// (`http::run_up`) then actually detaches: aborts its listeners, re-execs a
/// background copy of the server, and exits so the shell prompt comes back.
/// `Ctrl+C` exits the process directly (stopping the server for good), since
/// raw mode swallows the OS SIGINT that would normally do that.
pub async fn run(
    mut data: StatusData,
    dashboard_url: Option<String>,
    mcp_url: String,
    events: broadcast::Sender<serde_json::Value>,
    store: std::sync::Arc<SqliteStore>,
) -> Result<()> {
    let guard = TerminalGuard::enter(VIEWPORT_HEIGHT)?;
    let mut terminal = guard.terminal()?;
    let mut rx = events.subscribe();
    let mut feed: VecDeque<String> = VecDeque::with_capacity(MAX_FEED_LINES);
    let no_color = crate::tui::no_color();

    loop {
        terminal.draw(|frame| draw(&data, no_color, &dashboard_url, &mcp_url, &feed, frame))?;

        tokio::select! {
            event = rx.recv() => {
                if let Ok(value) = event {
                    if value.get("type").and_then(|t| t.as_str()) == Some("changed") {
                        // Re-fetch the real count rather than blindly incrementing:
                        // a "changed" event fires whenever data_version moves for
                        // any reason (delete, bulk import, several writes between
                        // poll ticks), not only "one memory was added". On failure,
                        // keep the last known-good count displayed rather than
                        // showing a wrong number.
                        match store.count().await {
                            Ok(count) => data.memory_count = count,
                            Err(e) => tracing::debug!("memory count refresh failed: {e:#}"),
                        }
                    }
                    let ts = chrono_now_hms();
                    let kind = value
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("event");
                    feed.push_front(format!("{ts}  {kind}"));
                    feed.truncate(MAX_FEED_LINES);
                }
            }
            key = poll_key_event() => {
                if let Some(key) = key {
                    match key.code {
                        KeyCode::Char('d') => break,
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            drop(guard);
                            let _ = std::fs::remove_file(crate::db::up_pidfile_path());
                            std::process::exit(0);
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    Ok(())
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

fn chrono_now_hms() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn draw(
    data: &StatusData,
    no_color: bool,
    dashboard_url: &Option<String>,
    mcp_url: &str,
    feed: &VecDeque<String>,
    frame: &mut ratatui::Frame,
) {
    let mut lines = vec![
        Line::from("Status     running"),
        Line::from(format!(
            "Server     http://{}:{}",
            data.server_host, data.server_port
        )),
        Line::from(format!("MCP        {mcp_url}")),
    ];
    if let Some(url) = dashboard_url {
        // Plain text, not an OSC 8 escape: ratatui's Buffer drops zero-width
        // control-character graphemes (including ESC) when building cells,
        // so raw hyperlink escapes never reach the terminal here. Most
        // terminals (iTerm2, kitty, Windows Terminal, VTE-based terminals)
        // auto-detect and linkify bare http(s) URLs on their own.
        let mut link_style = Style::default().add_modifier(Modifier::UNDERLINED);
        if !no_color {
            link_style = link_style.fg(CYAN);
        }
        lines.push(Line::from(vec![
            Span::raw("Dashboard  "),
            Span::styled(url.clone(), link_style),
        ]));
    }
    lines.push(Line::from(""));
    let feed_style = if no_color {
        Style::default()
    } else {
        Style::default().fg(CYAN)
    };
    for entry in feed.iter().take(20) {
        lines.push(Line::from(entry.as_str()).style(feed_style));
    }

    // Size the box to the widest line instead of a fixed width, so long
    // values (dashboard URL, feed entries) fit without clipping.
    let content_width = lines
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(FOOTER_TEXT.len()) as u16;

    let area = frame.area();
    let width = (content_width + BODY_FRAME_OVERHEAD)
        .max(MIN_BOX_WIDTH)
        .min(area.width);
    let area = Layout::horizontal([Constraint::Length(width), Constraint::Min(0)]).split(area)[0];
    let layout = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(data, no_color, layout[0], frame.buffer_mut());

    let body = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::new(2, 2, 0, 0))
        .title(" Activity ");
    let inner = body.inner(layout[1]);
    frame.render_widget(body, layout[1]);
    frame.render_widget(Paragraph::new(lines), inner);

    let footer_style = if no_color {
        Style::default()
    } else {
        Style::default().fg(DIM)
    };
    frame.render_widget(
        Paragraph::new(Line::from(FOOTER_TEXT).style(footer_style)),
        layout[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{DiscordStatusLine, MatrixStatusLine};
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

    fn render(
        data: &StatusData,
        no_color: bool,
        dashboard_url: &Option<String>,
        mcp_url: &str,
        feed: &VecDeque<String>,
    ) -> String {
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| draw(data, no_color, dashboard_url, mcp_url, feed, frame))
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
    fn draw_shows_server_url_and_mcp_url() {
        let data = sample_data();
        let feed = VecDeque::new();
        let content = render(&data, false, &None, "http://127.0.0.1:3456/mcp", &feed);
        assert!(content.contains("http://127.0.0.1:3456"));
        assert!(content.contains("http://127.0.0.1:3456/mcp"));
        assert!(content.contains("Status     running"));
    }

    #[test]
    fn draw_omits_dashboard_line_when_none() {
        let data = sample_data();
        let feed = VecDeque::new();
        let content = render(&data, false, &None, "http://127.0.0.1:3456/mcp", &feed);
        assert!(!content.contains("Dashboard"));
    }

    #[test]
    fn draw_shows_dashboard_url_when_some() {
        let data = sample_data();
        let feed = VecDeque::new();
        let dashboard_url = Some("http://127.0.0.1:3457".to_string());
        let content = render(
            &data,
            false,
            &dashboard_url,
            "http://127.0.0.1:3456/mcp",
            &feed,
        );
        assert!(content.contains("Dashboard"));
        assert!(content.contains("http://127.0.0.1:3457"));
    }

    #[test]
    fn draw_shows_feed_entries_most_recent_first() {
        let data = sample_data();
        let mut feed = VecDeque::new();
        feed.push_front("00:00:01  changed".to_string());
        let content = render(&data, false, &None, "http://127.0.0.1:3456/mcp", &feed);
        assert!(content.contains("00:00:01"));
        assert!(content.contains("changed"));
    }

    #[test]
    fn draw_truncates_feed_to_twenty_lines() {
        let data = sample_data();
        let mut feed = VecDeque::new();
        for i in 0..30 {
            feed.push_front(format!("entry-{i:02}"));
        }
        // Backend is only 20 rows tall in total (header + body + footer all
        // share it), so this just exercises the `.take(20)` path without
        // panicking or overflowing the render — the real assertion is that
        // rendering that many feed lines completes without error.
        let content = render(&data, false, &None, "http://127.0.0.1:3456/mcp", &feed);
        assert!(content.contains("entry-29"));
    }

    #[test]
    fn draw_footer_shows_detach_and_stop_hints() {
        let data = sample_data();
        let feed = VecDeque::new();
        let content = render(&data, false, &None, "http://127.0.0.1:3456/mcp", &feed);
        assert!(content.contains("d detach"));
        assert!(content.contains("ctrl+c stop server"));
    }

    #[test]
    fn draw_no_color_skips_foreground_styling_but_keeps_text() {
        let data = sample_data();
        let feed = VecDeque::new();
        let dashboard_url = Some("http://127.0.0.1:3457".to_string());
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    &data,
                    true,
                    &dashboard_url,
                    "http://127.0.0.1:3456/mcp",
                    &feed,
                    frame,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();
        assert!(content.contains("http://127.0.0.1:3457"));
        for cell in buffer.content.iter() {
            assert_eq!(
                cell.fg,
                Color::Reset,
                "cell {:?} should have no foreground color set when no_color=true",
                cell.symbol()
            );
        }
    }

    #[test]
    fn chrono_now_hms_formats_as_hh_mm_ss() {
        let ts = chrono_now_hms();
        assert_eq!(ts.len(), 8, "expected HH:MM:SS, got: {ts}");
        let parts: Vec<&str> = ts.split(':').collect();
        assert_eq!(parts.len(), 3);
        for part in parts {
            assert_eq!(part.len(), 2);
            assert!(part.chars().all(|c| c.is_ascii_digit()));
        }
    }
}
