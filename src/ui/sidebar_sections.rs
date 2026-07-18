//! Expanded-sidebar rendering for configured API-fed sections.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::api::schema::{SectionRow, SectionSpan};
use crate::app::state::{AppState, Palette};
use crate::config::{CustomSidebarSectionConfig, SidebarSectionPlacement};

use super::text::{display_width_u16, truncate_end};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SidebarSectionsLayout {
    pub agent_area: Rect,
    pub sections_area: Rect,
}

pub(crate) fn sidebar_sections_layout(app: &AppState, detail_area: Rect) -> SidebarSectionsLayout {
    let requested_height = app
        .sidebar_sections_config
        .iter()
        .map(|config| configured_section_height(app, config))
        .fold(0u16, u16::saturating_add);
    let available_height = detail_area
        .height
        .saturating_sub(super::sidebar::AGENT_PANEL_HEADER_ROWS);
    let sections_height = requested_height.min(available_height);
    let agent_height = detail_area.height.saturating_sub(sections_height);
    let agent_area = Rect::new(
        detail_area.x,
        detail_area.y,
        detail_area.width,
        agent_height,
    );
    let sections_area = Rect::new(
        detail_area.x,
        detail_area.y.saturating_add(agent_height),
        detail_area.width,
        sections_height,
    );
    SidebarSectionsLayout {
        agent_area,
        sections_area,
    }
}

pub(crate) fn expanded_sidebar_toggle_rect_for_state(app: &AppState, area: Rect) -> Rect {
    let fallback = super::sidebar::expanded_sidebar_toggle_rect(area);
    let (_, detail_area) =
        super::sidebar::expanded_sidebar_sections(area, app.sidebar_section_split);
    let layout = sidebar_sections_layout(app, detail_area);
    if layout.sections_area.height == 0 || layout.agent_area.height == 0 {
        return fallback;
    }
    Rect::new(
        fallback.x,
        layout
            .agent_area
            .y
            .saturating_add(layout.agent_area.height.saturating_sub(1)),
        fallback.width,
        fallback.height,
    )
}

fn configured_section_height(app: &AppState, config: &CustomSidebarSectionConfig) -> u16 {
    match config.placement {
        SidebarSectionPlacement::BelowAgents => app
            .sidebar_section_reports
            .rows(&config.id)
            .map(|rows| {
                (rows.len().min(config.max_rows as usize) as u16)
                    .saturating_add(u16::from(config.title.is_some()))
            })
            .unwrap_or(0),
    }
}

pub(super) fn render_sidebar_sections(app: &AppState, frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let bottom = area.y.saturating_add(area.height);
    let mut row_y = area.y;
    for config in &app.sidebar_sections_config {
        let Some(rows) = app.sidebar_section_reports.rows(&config.id) else {
            continue;
        };
        if let Some(title) = config.title.as_deref() {
            if row_y >= bottom {
                break;
            }
            frame.render_widget(
                Paragraph::new(Span::styled(
                    format!(" {}", title.to_uppercase()),
                    Style::default()
                        .fg(app.palette.overlay0)
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::DIM),
                )),
                Rect::new(area.x, row_y, area.width, 1),
            );
            row_y = row_y.saturating_add(1);
        }
        for row in rows.iter().take(config.max_rows as usize) {
            if row_y >= bottom {
                return;
            }
            render_section_row(
                frame,
                Rect::new(area.x, row_y, area.width, 1),
                row,
                &app.palette,
            );
            row_y = row_y.saturating_add(1);
        }
    }
}

fn render_section_row(frame: &mut Frame, area: Rect, row: &SectionRow, palette: &Palette) {
    match row {
        SectionRow::Spans(spans) => render_spans(frame, area, spans, palette),
        SectionRow::Bar { fraction, label } => {
            render_bar(frame, area, *fraction, label.as_deref(), palette)
        }
    }
}

fn render_spans(frame: &mut Frame, area: Rect, spans: &[SectionSpan], palette: &Palette) {
    let spans = spans
        .iter()
        .map(|span| {
            let mut style = Style::default().fg(section_color(span.color.as_deref(), palette));
            if span.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if span.dim {
                style = style.add_modifier(Modifier::DIM);
            }
            Span::styled(span.text.as_str(), style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_bar(
    frame: &mut Frame,
    area: Rect,
    fraction: f64,
    label: Option<&str>,
    palette: &Palette,
) {
    if area.width == 0 {
        return;
    }

    let label = label
        .map(|label| truncate_end(label, area.width.saturating_sub(1) as usize))
        .unwrap_or_default();
    let label_width = display_width_u16(&label).min(area.width);
    let gap = u16::from(label_width > 0 && area.width > label_width);
    let bar_width = area.width.saturating_sub(label_width.saturating_add(gap));
    let filled = ((fraction.clamp(0.0, 1.0) * f64::from(bar_width)).round() as u16).min(bar_width);

    let buffer = frame.buffer_mut();
    for offset in 0..bar_width {
        let cell = &mut buffer[(area.x + offset, area.y)];
        if offset < filled {
            let position = if bar_width <= 1 {
                0.0
            } else {
                f64::from(offset) / f64::from(bar_width - 1)
            };
            cell.set_symbol("█");
            cell.set_style(Style::default().fg(lerp_color(palette.green, palette.red, position)));
        } else {
            cell.set_symbol("░");
            cell.set_style(
                Style::default()
                    .fg(palette.surface_dim)
                    .add_modifier(Modifier::DIM),
            );
        }
    }

    if label_width > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(palette.subtext0)))
                .alignment(Alignment::Right),
            Rect::new(
                area.x + area.width.saturating_sub(label_width),
                area.y,
                label_width,
                1,
            ),
        );
    }
}

fn section_color(color: Option<&str>, palette: &Palette) -> Color {
    match color {
        None | Some("text") => palette.text,
        Some("accent") => palette.accent,
        Some("subtext0") => palette.subtext0,
        Some("green") => palette.green,
        Some("yellow") => palette.yellow,
        Some("red") => palette.red,
        Some("blue") => palette.blue,
        Some("mauve") => palette.mauve,
        Some("peach") => palette.peach,
        Some(color) => rgb_color(color).unwrap_or(palette.text),
    }
}

fn rgb_color(color: &str) -> Option<Color> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    Some(Color::Rgb(
        u8::from_str_radix(hex.get(0..2)?, 16).ok()?,
        u8::from_str_radix(hex.get(2..4)?, 16).ok()?,
        u8::from_str_radix(hex.get(4..6)?, 16).ok()?,
    ))
}

fn lerp_color(start: Color, end: Color, position: f64) -> Color {
    let (Color::Rgb(start_r, start_g, start_b), Color::Rgb(end_r, end_g, end_b)) = (start, end)
    else {
        return if position < 0.5 { start } else { end };
    };
    let lerp = |start: u8, end: u8| {
        (f64::from(start) + (f64::from(end) - f64::from(start)) * position).round() as u8
    };
    Color::Rgb(
        lerp(start_r, end_r),
        lerp(start_g, end_g),
        lerp(start_b, end_b),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CustomSidebarSectionConfig, SidebarSectionPlacement};
    use crate::workspace::Workspace;
    use ratatui::{backend::TestBackend, Terminal};
    use std::time::{Duration, Instant};

    fn config(id: &str, title: Option<&str>, max_rows: u16) -> CustomSidebarSectionConfig {
        CustomSidebarSectionConfig {
            id: id.into(),
            title: title.map(str::to_string),
            max_rows,
            placement: SidebarSectionPlacement::BelowAgents,
        }
    }

    fn span_row(text: &str) -> SectionRow {
        SectionRow::Spans(vec![SectionSpan {
            text: text.into(),
            color: None,
            bold: false,
            dim: false,
        }])
    }

    fn report(app: &mut AppState, id: &str, rows: Vec<SectionRow>) {
        assert_eq!(
            app.sidebar_section_reports
                .report(id.into(), "test", None, None, rows, Instant::now()),
            Ok(true)
        );
    }

    fn row_text(buffer: &ratatui::buffer::Buffer, row: u16, width: u16) -> String {
        (0..width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn section_with_title_renders_at_bottom_caps_rows_and_shrinks_agents() {
        let mut app = AppState::test_new();
        app.workspaces = (1..=6)
            .map(|index| Workspace::test_new(&format!("agent-{index}")))
            .collect();
        app.ensure_test_terminals();
        for workspace in &app.workspaces {
            let pane_id = workspace.tabs[0].root_pane;
            let terminal_id = workspace.tabs[0].panes[&pane_id]
                .attached_terminal_id
                .clone();
            app.terminals.get_mut(&terminal_id).unwrap().detected_agent =
                Some(crate::detect::Agent::Pi);
        }
        app.sidebar_agents.rows = vec![vec![crate::config::AgentSidebarToken::Workspace]];
        app.sidebar_sections_config = vec![config("build", Some("build status"), 1)];
        report(
            &mut app,
            "build",
            vec![span_row("ready"), span_row("capped")],
        );

        let area = Rect::new(0, 0, 20, 20);
        let (_, detail_area) =
            super::super::sidebar::expanded_sidebar_sections(area, app.sidebar_section_split);
        let full_metrics = super::super::sidebar::agent_panel_scroll_metrics(&app, detail_area);
        let layout = sidebar_sections_layout(&app, detail_area);
        let reduced_metrics =
            super::super::sidebar::agent_panel_scroll_metrics(&app, layout.agent_area);
        assert_eq!(layout.sections_area.height, 2);
        assert_eq!(layout.agent_area.height, detail_area.height - 2);
        assert_eq!(full_metrics.viewport_rows, 6);
        assert_eq!(reduced_metrics.viewport_rows, 5);

        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                super::super::sidebar::render_sidebar(
                    &app,
                    &crate::terminal::TerminalRuntimeRegistry::new(),
                    frame,
                    area,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            row_text(buffer, layout.sections_area.y, layout.sections_area.width),
            " BUILD STATUS"
        );
        assert_eq!(
            row_text(
                buffer,
                layout.sections_area.y + 1,
                layout.sections_area.width
            ),
            "ready"
        );
        let agents = (layout.agent_area.y..layout.agent_area.y + layout.agent_area.height)
            .map(|row| row_text(buffer, row, layout.agent_area.width))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!agents.contains("agent-6"), "rendered agents: {agents}");
    }

    #[test]
    fn section_block_is_absent_when_empty_expired_or_unconfigured() {
        let detail_area = Rect::new(0, 0, 20, 10);
        let mut app = AppState::test_new();
        app.sidebar_sections_config = vec![config("build", Some("build"), 6)];
        assert_eq!(
            sidebar_sections_layout(&app, detail_area)
                .sections_area
                .height,
            0
        );

        report(&mut app, "build", Vec::new());
        assert_eq!(
            sidebar_sections_layout(&app, detail_area)
                .sections_area
                .height,
            0
        );

        let now = Instant::now();
        assert_eq!(
            app.sidebar_section_reports.report(
                "build".into(),
                "test",
                None,
                Some(Duration::from_millis(1)),
                vec![span_row("temporary")],
                now,
            ),
            Ok(true)
        );
        assert!(app
            .sidebar_section_reports
            .expire_at(now + Duration::from_millis(1)));
        assert_eq!(
            sidebar_sections_layout(&app, detail_area)
                .sections_area
                .height,
            0
        );

        report(&mut app, "unconfigured", vec![span_row("hidden")]);
        app.sidebar_sections_config.clear();
        assert_eq!(
            sidebar_sections_layout(&app, detail_area)
                .sections_area
                .height,
            0
        );
    }

    #[test]
    fn bar_fill_cell_count_matches_fraction_boundaries() {
        let app = AppState::test_new();
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| {
                for (row, fraction) in [0.0, 0.5, 1.0].into_iter().enumerate() {
                    render_bar(
                        frame,
                        Rect::new(0, row as u16, 10, 1),
                        fraction,
                        None,
                        &app.palette,
                    );
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        for (row, expected) in [(0, 0), (1, 5), (2, 10)] {
            let filled = (0..10)
                .filter(|column| buffer[(*column, row)].symbol() == "█")
                .count();
            assert_eq!(filled, expected, "bar row {row}");
        }
        assert_eq!(buffer[(0, 2)].style().fg, Some(app.palette.green));
        assert_eq!(buffer[(9, 2)].style().fg, Some(app.palette.red));
    }

    #[test]
    fn span_rows_resolve_named_and_rgb_colors_with_modifiers() {
        let app = AppState::test_new();
        let row = SectionRow::Spans(vec![
            SectionSpan {
                text: "A".into(),
                color: Some("accent".into()),
                bold: true,
                dim: false,
            },
            SectionSpan {
                text: "B".into(),
                color: Some("#123456".into()),
                bold: false,
                dim: true,
            },
        ]);
        let mut terminal = Terminal::new(TestBackend::new(2, 1)).unwrap();
        terminal
            .draw(|frame| render_section_row(frame, Rect::new(0, 0, 2, 1), &row, &app.palette))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].style().fg, Some(app.palette.accent));
        assert!(buffer[(0, 0)].style().add_modifier.contains(Modifier::BOLD));
        assert_eq!(
            buffer[(1, 0)].style().fg,
            Some(Color::Rgb(0x12, 0x34, 0x56))
        );
        assert!(buffer[(1, 0)].style().add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn custom_sections_do_not_change_collapsed_rendering() {
        let mut app = AppState::test_new();
        app.workspaces = vec![Workspace::test_new("one")];
        app.ensure_test_terminals();
        let baseline = app;
        let mut configured = AppState::test_new();
        configured.workspaces = vec![Workspace::test_new("one")];
        configured.ensure_test_terminals();
        configured.sidebar_sections_config = vec![config("build", Some("build"), 6)];
        report(&mut configured, "build", vec![span_row("ready")]);
        let area = Rect::new(0, 0, 4, 12);

        let mut baseline_terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        baseline_terminal
            .draw(|frame| super::super::sidebar::render_sidebar_collapsed(&baseline, frame, area))
            .unwrap();
        let mut configured_terminal =
            Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        configured_terminal
            .draw(|frame| super::super::sidebar::render_sidebar_collapsed(&configured, frame, area))
            .unwrap();

        assert_eq!(
            baseline_terminal.backend().buffer(),
            configured_terminal.backend().buffer()
        );
    }
}
