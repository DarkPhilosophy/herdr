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

    let requested_height = app
        .sidebar_sections_config
        .iter()
        .map(|config| configured_section_height(app, config))
        .fold(0u16, u16::saturating_add);
    let height_overflow = requested_height > area.height;
    let mut remaining = area.height.saturating_sub(u16::from(height_overflow));
    let total_live_rows = app
        .sidebar_sections_config
        .iter()
        .filter_map(|config| app.sidebar_section_reports.rows(&config.id))
        .map(<[SectionRow]>::len)
        .sum::<usize>();
    let mut represented_rows = 0usize;
    let mut row_y = area.y;

    'sections: for config in &app.sidebar_sections_config {
        let Some(rows) = app.sidebar_section_reports.rows(&config.id) else {
            continue;
        };
        if let Some(title) = config.title.as_deref() {
            if remaining == 0 {
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
            remaining = remaining.saturating_sub(1);
        }

        let max_rows = config.max_rows as usize;
        let capped = rows.len() > max_rows;
        let content_rows = if capped {
            max_rows.saturating_sub(1)
        } else {
            rows.len()
        };
        for row in rows.iter().take(content_rows) {
            if remaining == 0 {
                break 'sections;
            }
            render_section_row(
                frame,
                Rect::new(area.x, row_y, area.width, 1),
                row,
                &app.palette,
            );
            represented_rows += 1;
            row_y = row_y.saturating_add(1);
            remaining = remaining.saturating_sub(1);
        }
        if capped {
            if remaining == 0 {
                break;
            }
            let hidden = rows.len().saturating_sub(content_rows);
            render_overflow_indicator(
                frame,
                Rect::new(area.x, row_y, area.width, 1),
                hidden,
                &app.palette,
            );
            represented_rows = represented_rows.saturating_add(hidden);
            row_y = row_y.saturating_add(1);
            remaining = remaining.saturating_sub(1);
        }
    }

    if height_overflow {
        render_overflow_indicator(
            frame,
            Rect::new(
                area.x,
                area.y.saturating_add(area.height.saturating_sub(1)),
                area.width,
                1,
            ),
            total_live_rows.saturating_sub(represented_rows),
            &app.palette,
        );
    }
}

fn render_overflow_indicator(frame: &mut Frame, area: Rect, hidden: usize, palette: &Palette) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!("… {hidden} more"),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::DIM),
        )),
        area,
    );
}

fn render_section_row(frame: &mut Frame, area: Rect, row: &SectionRow, palette: &Palette) {
    match row {
        SectionRow::Spans { spans, right } => {
            render_spans(frame, area, spans, right, palette);
        }
        SectionRow::Bar { bar } => render_bar(
            frame,
            area,
            bar.fraction,
            bar.label.as_deref(),
            bar.fill.as_deref(),
            bar.empty.as_deref(),
            palette,
        ),
    }
}

fn render_spans(
    frame: &mut Frame,
    area: Rect,
    spans: &[SectionSpan],
    right: &[SectionSpan],
    palette: &Palette,
) {
    let right_width = right
        .iter()
        .map(|span| display_width_u16(&span.text))
        .fold(0u16, u16::saturating_add)
        .min(area.width);
    let left_has_content = spans.iter().any(|span| display_width_u16(&span.text) > 0);
    let gap = u16::from(left_has_content && right_width > 0 && area.width > right_width);
    let left_width = area.width.saturating_sub(right_width.saturating_add(gap));

    if left_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(styled_spans(spans, palette))),
            Rect::new(area.x, area.y, left_width, 1),
        );
    }
    if right_width > 0 {
        frame.render_widget(
            Paragraph::new(Line::from(styled_spans(right, palette))).alignment(Alignment::Right),
            Rect::new(
                area.x
                    .saturating_add(area.width.saturating_sub(right_width)),
                area.y,
                right_width,
                1,
            ),
        );
    }
}

fn styled_spans<'a>(spans: &'a [SectionSpan], palette: &Palette) -> Vec<Span<'a>> {
    spans
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
        .collect()
}

fn render_bar(
    frame: &mut Frame,
    area: Rect,
    fraction: f64,
    label: Option<&str>,
    fill: Option<&str>,
    empty: Option<&str>,
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
            let color = fill.map_or_else(
                || {
                    let position = if bar_width <= 1 {
                        0.0
                    } else {
                        f64::from(offset) / f64::from(bar_width - 1)
                    };
                    lerp_color(palette.green, palette.red, position)
                },
                |fill| section_color(Some(fill), palette),
            );
            cell.set_symbol("█");
            cell.set_style(Style::default().fg(color));
        } else {
            cell.set_symbol("░");
            let style = empty.map_or_else(
                || {
                    Style::default()
                        .fg(palette.surface_dim)
                        .add_modifier(Modifier::DIM)
                },
                |empty| Style::default().fg(section_color(Some(empty), palette)),
            );
            cell.set_style(style);
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
    use crate::api::schema::SectionBar;
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
        SectionRow::Spans {
            spans: vec![SectionSpan {
                text: text.into(),
                color: None,
                bold: false,
                dim: false,
            }],
            right: Vec::new(),
        }
    }

    fn split_span_row(left: &str, right: &str) -> SectionRow {
        SectionRow::Spans {
            spans: vec![SectionSpan {
                text: left.into(),
                color: None,
                bold: false,
                dim: false,
            }],
            right: vec![SectionSpan {
                text: right.into(),
                color: Some("subtext0".into()),
                bold: false,
                dim: true,
            }],
        }
    }

    fn bar_row() -> SectionRow {
        SectionRow::Bar {
            bar: SectionBar {
                fraction: 0.5,
                label: None,
                fill: Some("green".into()),
                empty: Some("subtext0".into()),
            },
        }
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
    fn section_with_title_renders_max_rows_overflow_at_bottom_and_shrinks_agents() {
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
        app.sidebar_sections_config = vec![config("build", Some("build status"), 2)];
        report(
            &mut app,
            "build",
            vec![span_row("ready"), span_row("capped"), span_row("hidden")],
        );

        let area = Rect::new(0, 0, 20, 20);
        let (_, detail_area) =
            super::super::sidebar::expanded_sidebar_sections(area, app.sidebar_section_split);
        let full_metrics = super::super::sidebar::agent_panel_scroll_metrics(&app, detail_area);
        let layout = sidebar_sections_layout(&app, detail_area);
        let reduced_metrics =
            super::super::sidebar::agent_panel_scroll_metrics(&app, layout.agent_area);
        assert_eq!(layout.sections_area.height, 3);
        assert_eq!(layout.agent_area.height, detail_area.height - 3);
        assert!(reduced_metrics.viewport_rows < full_metrics.viewport_rows);

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
        assert_eq!(
            row_text(
                buffer,
                layout.sections_area.y + 2,
                layout.sections_area.width
            ),
            "… 2 more"
        );
        assert!(buffer[(layout.sections_area.x, layout.sections_area.y + 2)]
            .style()
            .add_modifier
            .contains(Modifier::DIM));
        let agents = (layout.agent_area.y..layout.agent_area.y + layout.agent_area.height)
            .map(|row| row_text(buffer, row, layout.agent_area.width))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!agents.contains("agent-6"), "rendered agents: {agents}");
    }

    #[test]
    fn single_account_payload_renders_all_rows_and_reports_height_overflow() {
        let mut rows = Vec::with_capacity(11);
        rows.push(span_row("ACCOUNT"));
        for window in 1..=5 {
            rows.push(split_span_row(
                &format!("window-{window}"),
                &format!("{}% 5m", window * 10),
            ));
            rows.push(bar_row());
        }
        assert_eq!(rows.len(), 11);

        let mut app = AppState::test_new();
        app.sidebar_sections_config = vec![config("account", None, 12)];
        report(&mut app, "account", rows);

        let roomy_area = Rect::new(0, 0, 20, 30);
        let roomy_layout = sidebar_sections_layout(&app, roomy_area);
        assert_eq!(roomy_layout.sections_area.height, 11);
        let mut roomy_terminal =
            Terminal::new(TestBackend::new(roomy_area.width, roomy_area.height)).unwrap();
        roomy_terminal
            .draw(|frame| render_sidebar_sections(&app, frame, roomy_layout.sections_area))
            .unwrap();
        let roomy_rows = (roomy_layout.sections_area.y
            ..roomy_layout.sections_area.y + roomy_layout.sections_area.height)
            .map(|row| row_text(roomy_terminal.backend().buffer(), row, roomy_area.width))
            .collect::<Vec<_>>();
        assert_eq!(roomy_rows.len(), 11);
        assert!(roomy_rows.iter().all(|row| !row.is_empty()));
        assert_eq!(roomy_rows[0], "ACCOUNT");
        assert!(roomy_rows[1].starts_with("window-1"));
        assert!(roomy_rows[1].ends_with("10% 5m"));
        assert_eq!(display_width_u16(&roomy_rows[2]), roomy_area.width);
        assert!(roomy_rows.iter().all(|row| !row.contains("more")));

        let cramped_area = Rect::new(0, 0, 20, 10);
        let cramped_layout = sidebar_sections_layout(&app, cramped_area);
        assert_eq!(cramped_layout.agent_area.height, 3);
        assert_eq!(cramped_layout.sections_area.height, 7);
        let mut cramped_terminal =
            Terminal::new(TestBackend::new(cramped_area.width, cramped_area.height)).unwrap();
        cramped_terminal
            .draw(|frame| render_sidebar_sections(&app, frame, cramped_layout.sections_area))
            .unwrap();
        let overflow_y = cramped_layout
            .sections_area
            .y
            .saturating_add(cramped_layout.sections_area.height - 1);
        assert_eq!(
            row_text(
                cramped_terminal.backend().buffer(),
                overflow_y,
                cramped_area.width
            ),
            "… 5 more"
        );
        assert!(
            cramped_terminal.backend().buffer()[(cramped_layout.sections_area.x, overflow_y)]
                .style()
                .add_modifier
                .contains(Modifier::DIM)
        );
    }

    #[test]
    fn twenty_four_row_boundary_renders_without_overflow() {
        let mut app = AppState::test_new();
        app.sidebar_sections_config = vec![config("boundary", None, 24)];
        report(
            &mut app,
            "boundary",
            (0..24)
                .map(|index| span_row(&format!("row-{index:02}")))
                .collect(),
        );

        let area = Rect::new(0, 0, 20, 27);
        let layout = sidebar_sections_layout(&app, area);
        assert_eq!(layout.sections_area.height, 24);
        assert_eq!(layout.agent_area.height, 3);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| render_sidebar_sections(&app, frame, layout.sections_area))
            .unwrap();
        let rendered = (0..24)
            .map(|offset| {
                row_text(
                    terminal.backend().buffer(),
                    layout.sections_area.y + offset,
                    area.width,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered.first().map(String::as_str), Some("row-00"));
        assert_eq!(rendered.last().map(String::as_str), Some("row-23"));
        assert!(rendered.iter().all(|row| !row.contains("more")));
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

        assert_eq!(
            app.sidebar_section_reports.report(
                "build".into(),
                "test",
                None,
                None,
                Vec::new(),
                Instant::now(),
            ),
            Ok(false)
        );
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
                        None,
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
    fn bar_color_overrides_use_uniform_fill_and_custom_empty_color() {
        let app = AppState::test_new();
        let mut terminal = Terminal::new(TestBackend::new(10, 2)).unwrap();
        terminal
            .draw(|frame| {
                render_bar(
                    frame,
                    Rect::new(0, 0, 10, 1),
                    0.5,
                    None,
                    Some("#123456"),
                    None,
                    &app.palette,
                );
                render_bar(
                    frame,
                    Rect::new(0, 1, 10, 1),
                    0.5,
                    None,
                    None,
                    Some("mauve"),
                    &app.palette,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        for column in 0..5 {
            assert_eq!(
                buffer[(column, 0)].style().fg,
                Some(Color::Rgb(0x12, 0x34, 0x56))
            );
        }
        for column in 5..10 {
            assert_eq!(buffer[(column, 1)].style().fg, Some(app.palette.mauve));
        }
    }

    #[test]
    fn span_rows_resolve_named_and_rgb_colors_with_modifiers() {
        let app = AppState::test_new();
        let row = SectionRow::Spans {
            spans: vec![
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
            ],
            right: Vec::new(),
        };
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
    fn right_span_cluster_wins_width_and_keeps_one_cell_gap() {
        let app = AppState::test_new();
        let row = split_span_row("abcdefghij", "R9");
        let mut terminal = Terminal::new(TestBackend::new(12, 1)).unwrap();
        terminal
            .draw(|frame| render_section_row(frame, Rect::new(0, 0, 12, 1), &row, &app.palette))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(
            (0..9)
                .map(|column| buffer[(column, 0)].symbol())
                .collect::<String>(),
            "abcdefghi"
        );
        assert_eq!(buffer[(9, 0)].symbol(), " ");
        assert_eq!(buffer[(10, 0)].symbol(), "R");
        assert_eq!(buffer[(11, 0)].symbol(), "9");
        for column in 10..12 {
            assert_eq!(buffer[(column, 0)].style().fg, Some(app.palette.subtext0));
            assert!(buffer[(column, 0)]
                .style()
                .add_modifier
                .contains(Modifier::DIM));
        }
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
