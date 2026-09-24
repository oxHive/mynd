use crate::cli::StatusData;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Widget as _},
};

const HEX_MARK: &str = "\u{25c7}"; // ◇, matches the outline-icon dashboard sidebar mark
const BRAND_PURPLE: Color = Color::Rgb(0xa2, 0x9b, 0xef);
const DIM: Color = Color::Rgb(0x8a, 0x8a, 0x9a);
const CYAN: Color = Color::Rgb(0x67, 0xe8, 0xf9);
const WARNING: Color = Color::Rgb(0xfb, 0xbf, 0x24);

fn dim(no_color: bool) -> Style {
    if no_color {
        Style::default()
    } else {
        Style::default().fg(DIM)
    }
}

/// Pure render function: same inputs always produce the same cells. Shared
/// by the status and up TUI views so the header looks identical in both.
/// `no_color` strips foreground colors (NO_COLOR convention) while keeping
/// the same layout, borders, and text.
pub fn render_header(data: &StatusData, no_color: bool, area: Rect, buf: &mut Buffer) {
    let mut brand_style = Style::default().add_modifier(Modifier::BOLD);
    if !no_color {
        brand_style = brand_style.fg(BRAND_PURPLE);
    }
    let mut title = vec![
        Span::raw(" "),
        Span::styled(format!("{HEX_MARK} Mynd"), brand_style),
        Span::styled(format!(" v{}", data.version), dim(no_color)),
    ];
    if let Some(update) = &data.available_update {
        let mut update_style = Style::default();
        if !no_color {
            update_style = update_style.fg(WARNING);
        }
        title.push(Span::styled(
            format!(" (v{} available)", update.version),
            update_style,
        ));
    }
    title.push(Span::raw(" "));
    let block = Block::default()
        .borders(Borders::ALL)
        .padding(Padding::new(2, 2, 0, 0))
        .title(Line::from(title));
    let inner = block.inner(area);
    block.render(area, buf);

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);

    let project_line = Line::from(vec![
        Span::styled("Project  ", dim(no_color)),
        Span::raw(
            data.project_label
                .clone()
                .unwrap_or_else(|| "(none)".to_string()),
        ),
    ]);
    Paragraph::new(project_line).render(rows[0], buf);

    let mut count_style = Style::default();
    if !no_color {
        count_style = count_style.fg(CYAN);
    }
    let memories_line = Line::from(vec![
        Span::styled("Memories ", dim(no_color)),
        Span::styled(data.memory_count.to_string(), count_style),
        Span::styled(" stored", dim(no_color)),
    ]);
    Paragraph::new(memories_line).render(rows[1], buf);

    if let Some(project) = &data.project {
        let remaining = project.max_tokens.saturating_sub(project.used_tokens);
        let budget_line = Line::from(vec![
            Span::styled("Budget   ", dim(no_color)),
            Span::raw(format!(
                "{} / {} tokens injected ({} remaining)",
                project.used_tokens, project.max_tokens, remaining
            )),
        ]);
        Paragraph::new(budget_line).render(rows[2], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::StatusData;
    use ratatui::{Terminal, backend::TestBackend};

    fn sample_data() -> StatusData {
        StatusData {
            version: "0.6.0",
            available_update: None,
            project_label: Some("oxhive-mynd".to_string()),
            server_up: true,
            server_host: "127.0.0.1".to_string(),
            server_port: 3456,
            db_path: "~/.local/share/mynd/memories.db".to_string(),
            memory_count: 128,
            sync_enabled: false,
            sync_remote_url: String::new(),
            registered_clients: vec!["claude".to_string(), "opencode".to_string()],
            project: None,
            matrix: crate::cli::MatrixStatusLine::NotConfigured,
            discord: crate::cli::DiscordStatusLine::NotConfigured,
        }
    }

    #[test]
    fn header_shows_wordmark_and_project() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let data = sample_data();
        terminal
            .draw(|frame| render_header(&data, false, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Mynd"));
        assert!(content.contains("oxhive-mynd"));
        assert!(content.contains("128"));
    }

    #[test]
    fn header_marks_available_update_beside_version() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut data = sample_data();
        data.available_update = Some(crate::cli::AvailableUpdate {
            version: "0.7.0".to_string(),
            upgrade_hint: "mynd upgrade",
        });
        terminal
            .draw(|frame| render_header(&data, false, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("v0.6.0 (v0.7.0 available)"), "{content}");
    }

    #[test]
    fn header_shows_budget_line_when_project_present() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut data = sample_data();
        data.project = Some(crate::cli::ProjectStatus {
            project_name: "mynd".to_string(),
            has_local_config: false,
            file_open_rule_count: 0,
            mention_trigger_count: 0,
            loaded: vec![],
            skipped: vec![],
            used_tokens: 500,
            max_tokens: 2000,
            truncated: false,
        });
        terminal
            .draw(|frame| render_header(&data, false, frame.area(), frame.buffer_mut()))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("500"));
        assert!(content.contains("2000"));
        assert!(content.contains("1500 remaining"));
    }

    #[test]
    fn header_no_color_skips_foreground_styling_but_keeps_text() {
        let backend = TestBackend::new(80, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        let data = sample_data();

        terminal
            .draw(|frame| render_header(&data, true, frame.area(), frame.buffer_mut()))
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let content: String = buffer.content.iter().map(|c| c.symbol()).collect();

        // Text content is unchanged regardless of no_color.
        assert!(content.contains("Mynd"));
        assert!(content.contains("oxhive-mynd"));
        assert!(content.contains("128"));

        // No cell in the rendered buffer carries a foreground color when no_color is set.
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
