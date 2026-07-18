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
                    .saturating_add(1)
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
        let max_rows = config.max_rows as usize;
        let capped = rows.len() > max_rows;
        let content_rows = if capped {
            max_rows.saturating_sub(1)
        } else {
            rows.len()
        };
        let bar_columns = BarColumns::for_rows(rows.iter().take(content_rows), area.width);

        if remaining == 0 {
            break;
        }
        render_section_divider(frame, Rect::new(area.x, row_y, area.width, 1), &app.palette);
        row_y = row_y.saturating_add(1);
        remaining = remaining.saturating_sub(1);

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

        for row in rows.iter().take(content_rows) {
            if remaining == 0 {
                break 'sections;
            }
            render_section_row(
                frame,
                Rect::new(area.x, row_y, area.width, 1),
                row,
                bar_columns,
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

fn render_section_divider(frame: &mut Frame, area: Rect, palette: &Palette) {
    let buffer = frame.buffer_mut();
    for x in area.x..area.x.saturating_add(area.width) {
        buffer[(x, area.y)].set_symbol("─");
        buffer[(x, area.y)].set_style(Style::default().fg(palette.surface_dim));
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

fn render_section_row(
    frame: &mut Frame,
    area: Rect,
    row: &SectionRow,
    bar_columns: BarColumns,
    palette: &Palette,
) {
    match row {
        SectionRow::Spans { spans, right } => {
            render_spans(frame, area, spans, right, palette);
        }
        SectionRow::Bar { bar } => render_bar(
            frame,
            area,
            bar.fraction,
            bar.title.as_deref(),
            bar.title_color.as_deref(),
            bar.label.as_deref(),
            bar.fill.as_deref(),
            bar.empty.as_deref(),
            bar_columns,
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
    if !left_has_content && right_width == 0 {
        let buffer = frame.buffer_mut();
        for x in area.x..area.x.saturating_add(area.width) {
            buffer[(x, area.y)].reset();
        }
        return;
    }
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

/// A bar row's point is the bar itself: reserve this many cells before
/// granting any width to the left title or the right label.
const MIN_BAR_CELLS: u16 = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct BarColumns {
    title_width: u16,
    label_width: u16,
}

impl BarColumns {
    fn for_rows<'a>(rows: impl Iterator<Item = &'a SectionRow>, area_width: u16) -> Self {
        let (title_width, label_width) = rows
            .filter_map(|row| match row {
                SectionRow::Bar { bar } => Some((
                    bar.title
                        .as_deref()
                        .map(display_width_u16)
                        .unwrap_or_default(),
                    bar.label
                        .as_deref()
                        .map(display_width_u16)
                        .unwrap_or_default(),
                )),
                SectionRow::Spans { .. } => None,
            })
            .fold((0, 0), |(max_title, max_label), (title, label)| {
                (max_title.max(title), max_label.max(label))
            });
        Self::fit(title_width, label_width, area_width)
    }

    fn fit(title_width: u16, label_width: u16, area_width: u16) -> Self {
        let side_budget = area_width.saturating_sub(MIN_BAR_CELLS.min(area_width));
        let title_footprint = column_footprint(title_width);
        let label_footprint = column_footprint(label_width);
        if title_footprint.saturating_add(label_footprint) <= side_budget {
            return Self {
                title_width,
                label_width,
            };
        }

        if label_footprint <= side_budget {
            return Self {
                title_width: fit_column_width(
                    title_width,
                    side_budget.saturating_sub(label_footprint),
                ),
                label_width,
            };
        }

        Self {
            title_width: 0,
            label_width: fit_column_width(label_width, side_budget),
        }
    }

    fn title_gap(self) -> u16 {
        u16::from(self.title_width > 0)
    }
}

fn column_footprint(width: u16) -> u16 {
    width.saturating_add(u16::from(width > 0))
}

fn fit_column_width(desired: u16, budget: u16) -> u16 {
    if desired == 0 || budget < 2 {
        0
    } else {
        desired.min(budget - 1)
    }
}

#[allow(clippy::too_many_arguments)]
fn render_bar(
    frame: &mut Frame,
    area: Rect,
    fraction: f64,
    title: Option<&str>,
    title_color: Option<&str>,
    label: Option<&str>,
    fill: Option<&str>,
    empty: Option<&str>,
    columns: BarColumns,
    palette: &Palette,
) {
    if area.width == 0 {
        return;
    }

    let title = title
        .map(|title| truncate_end(title, columns.title_width.into()))
        .unwrap_or_default();
    let label = label
        .map(|label| truncate_end(label, columns.label_width.into()))
        .unwrap_or_default();
    let bar_x = area
        .x
        .saturating_add(columns.title_width)
        .saturating_add(columns.title_gap());
    let bar_width = area
        .width
        .saturating_sub(column_footprint(columns.title_width))
        .saturating_sub(column_footprint(columns.label_width));
    let filled = ((fraction.clamp(0.0, 1.0) * f64::from(bar_width)).round() as u16).min(bar_width);

    let buffer = frame.buffer_mut();
    for offset in 0..bar_width {
        let cell = &mut buffer[(bar_x + offset, area.y)];
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

    if columns.title_width > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                title,
                Style::default().fg(section_color(title_color, palette)),
            )),
            Rect::new(area.x, area.y, columns.title_width, 1),
        );
    }
    if columns.label_width > 0 {
        frame.render_widget(
            Paragraph::new(Span::styled(label, Style::default().fg(palette.subtext0)))
                .alignment(Alignment::Right),
            Rect::new(
                area.x + area.width.saturating_sub(columns.label_width),
                area.y,
                columns.label_width,
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
                title: None,
                title_color: None,
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
    fn section_with_title_renders_styled_divider_and_max_rows_overflow() {
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
        assert_eq!(layout.sections_area.height, 4);
        assert_eq!(layout.agent_area.height, detail_area.height - 4);
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
            "─".repeat(layout.sections_area.width.into())
        );
        for x in layout.sections_area.x
            ..layout
                .sections_area
                .x
                .saturating_add(layout.sections_area.width)
        {
            assert_eq!(
                buffer[(x, layout.sections_area.y)].style().fg,
                Some(app.palette.surface_dim)
            );
        }
        assert_eq!(
            row_text(
                buffer,
                layout.sections_area.y + 1,
                layout.sections_area.width
            ),
            " BUILD STATUS"
        );
        assert_eq!(
            row_text(
                buffer,
                layout.sections_area.y + 2,
                layout.sections_area.width
            ),
            "ready"
        );
        assert_eq!(
            row_text(
                buffer,
                layout.sections_area.y + 3,
                layout.sections_area.width
            ),
            "… 2 more"
        );
        assert!(buffer[(layout.sections_area.x, layout.sections_area.y + 3)]
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
        assert_eq!(roomy_layout.sections_area.height, 12);
        let mut roomy_terminal =
            Terminal::new(TestBackend::new(roomy_area.width, roomy_area.height)).unwrap();
        roomy_terminal
            .draw(|frame| render_sidebar_sections(&app, frame, roomy_layout.sections_area))
            .unwrap();
        let roomy_rows = (roomy_layout.sections_area.y
            ..roomy_layout.sections_area.y + roomy_layout.sections_area.height)
            .map(|row| row_text(roomy_terminal.backend().buffer(), row, roomy_area.width))
            .collect::<Vec<_>>();
        assert_eq!(roomy_rows.len(), 12);
        assert!(roomy_rows.iter().all(|row| !row.is_empty()));
        assert_eq!(roomy_rows[0], "─".repeat(roomy_area.width.into()));
        assert_eq!(roomy_rows[1], "ACCOUNT");
        assert!(roomy_rows[2].starts_with("window-1"));
        assert!(roomy_rows[2].ends_with("10% 5m"));
        assert_eq!(display_width_u16(&roomy_rows[3]), roomy_area.width);
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
            "… 6 more"
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

        let area = Rect::new(0, 0, 20, 28);
        let layout = sidebar_sections_layout(&app, area);
        assert_eq!(layout.sections_area.height, 25);
        assert_eq!(layout.agent_area.height, 3);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| render_sidebar_sections(&app, frame, layout.sections_area))
            .unwrap();
        let rendered = (0..25)
            .map(|offset| {
                row_text(
                    terminal.backend().buffer(),
                    layout.sections_area.y + offset,
                    area.width,
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            rendered.first().map(String::as_str),
            Some("────────────────────")
        );
        assert_eq!(rendered.get(1).map(String::as_str), Some("row-00"));
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
    fn bar_title_renders_left_and_reserves_minimum_bar() {
        let app = AppState::test_new();
        let row0_columns = BarColumns::fit(
            display_width_u16("mum 5h"),
            display_width_u16("42% \u{21bb}1h47"),
            26,
        );
        let row1_columns = BarColumns::fit(
            display_width_u16("victor spark 5h window"),
            display_width_u16("100% \u{21bb}23h59"),
            26,
        );
        let mut terminal = Terminal::new(TestBackend::new(26, 2)).unwrap();
        terminal
            .draw(|frame| {
                render_bar(
                    frame,
                    Rect::new(0, 0, 26, 1),
                    0.5,
                    Some("mum 5h"),
                    None,
                    Some("42% \u{21bb}1h47"),
                    None,
                    None,
                    row0_columns,
                    &app.palette,
                );
                render_bar(
                    frame,
                    Rect::new(0, 1, 26, 1),
                    1.0,
                    Some("victor spark 5h window"),
                    None,
                    Some("100% \u{21bb}23h59"),
                    None,
                    None,
                    row1_columns,
                    &app.palette,
                );
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row0 = row_text(buffer, 0, 26);
        assert!(row0.starts_with("mum 5h "), "title leads the row: {row0}");
        assert!(
            row0.ends_with("42% \u{21bb}1h47"),
            "label ends the row: {row0}"
        );
        assert_eq!(
            row0.matches('█').count() + row0.matches('░').count(),
            9,
            "bar takes the middle budget: {row0}"
        );
        let row1 = row_text(buffer, 1, 26);
        assert!(
            row1.ends_with("100% \u{21bb}23h59"),
            "right label wins: {row1}"
        );
        assert_eq!(
            row1.matches('█').count(),
            MIN_BAR_CELLS as usize,
            "minimum bar cells reserved under a long title: {row1}"
        );
    }

    #[test]
    fn shared_bar_budget_shrinks_title_before_label() {
        assert_eq!(
            BarColumns::fit(8, 4, 12),
            BarColumns {
                title_width: 0,
                label_width: 4,
            }
        );
        let narrower = BarColumns::fit(8, 4, 10);
        assert_eq!(
            narrower,
            BarColumns {
                title_width: 0,
                label_width: 3,
            }
        );
        assert_eq!(
            10u16
                .saturating_sub(column_footprint(narrower.title_width))
                .saturating_sub(column_footprint(narrower.label_width)),
            MIN_BAR_CELLS
        );
    }

    #[test]
    fn section_bar_rows_share_columns_and_identical_middle_budgets() {
        let rows = [
            ("al* 5h", "1% \u{21bb}4h38m", 0.1),
            ("mu* sp 5h", "100% \u{21bb}23h59", 0.5),
            ("vi* fa", "80%", 0.8),
        ]
        .into_iter()
        .map(|(title, label, fraction)| SectionRow::Bar {
            bar: SectionBar {
                fraction,
                title: Some(title.into()),
                title_color: None,
                label: Some(label.into()),
                fill: None,
                empty: None,
            },
        })
        .collect::<Vec<_>>();
        let mut app = AppState::test_new();
        app.sidebar_sections_config = vec![config("usage", None, 3)];
        report(&mut app, "usage", rows);

        let area = Rect::new(0, 0, 42, 8);
        let layout = sidebar_sections_layout(&app, area);
        assert_eq!(layout.sections_area.height, 4);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| render_sidebar_sections(&app, frame, layout.sections_area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let bar_metrics = (1..=3)
            .map(|offset| {
                let row = layout.sections_area.y + offset;
                let positions = (0..area.width)
                    .filter(|column| matches!(buffer[(*column, row)].symbol(), "█" | "░"))
                    .collect::<Vec<_>>();
                (
                    positions.len(),
                    positions.first().copied(),
                    positions.last().copied(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(bar_metrics, vec![(20, Some(10), Some(29)); 3]);
        assert_eq!(
            (0..6)
                .map(|column| buffer[(column, layout.sections_area.y + 1)].symbol())
                .collect::<String>(),
            "al* 5h"
        );
        assert_eq!(
            (39..42)
                .map(|column| buffer[(column, layout.sections_area.y + 3)].symbol())
                .collect::<String>(),
            "80%"
        );
    }

    #[test]
    fn bar_titles_resolve_named_and_rgb_colors() {
        let rows = [
            SectionRow::Bar {
                bar: SectionBar {
                    fraction: 0.5,
                    title: Some("A".into()),
                    title_color: Some("peach".into()),
                    label: None,
                    fill: None,
                    empty: None,
                },
            },
            SectionRow::Bar {
                bar: SectionBar {
                    fraction: 0.5,
                    title: Some("B".into()),
                    title_color: Some("#123456".into()),
                    label: None,
                    fill: None,
                    empty: None,
                },
            },
        ];
        let columns = BarColumns::for_rows(rows.iter(), 12);
        let app = AppState::test_new();
        let mut terminal = Terminal::new(TestBackend::new(12, 2)).unwrap();
        terminal
            .draw(|frame| {
                for (row_index, row) in rows.iter().enumerate() {
                    render_section_row(
                        frame,
                        Rect::new(0, row_index as u16, 12, 1),
                        row,
                        columns,
                        &app.palette,
                    );
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].style().fg, Some(app.palette.peach));
        assert_eq!(
            buffer[(0, 1)].style().fg,
            Some(Color::Rgb(0x12, 0x34, 0x56))
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
                        None,
                        None,
                        BarColumns::default(),
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
                    None,
                    None,
                    Some("#123456"),
                    None,
                    BarColumns::default(),
                    &app.palette,
                );
                render_bar(
                    frame,
                    Rect::new(0, 1, 10, 1),
                    0.5,
                    None,
                    None,
                    None,
                    None,
                    Some("mauve"),
                    BarColumns::default(),
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
            .draw(|frame| {
                render_section_row(
                    frame,
                    Rect::new(0, 0, 2, 1),
                    &row,
                    BarColumns::default(),
                    &app.palette,
                )
            })
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
    fn blank_spans_row_renders_an_empty_line() {
        let app = AppState::test_new();
        let row = SectionRow::Spans {
            spans: Vec::new(),
            right: Vec::new(),
        };
        let area = Rect::new(0, 0, 8, 1);
        let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
        terminal
            .draw(|frame| {
                frame.render_widget(Paragraph::new("occupied"), area);
                render_section_row(frame, area, &row, BarColumns::default(), &app.palette);
            })
            .unwrap();
        assert!((0..area.width)
            .all(|column| terminal.backend().buffer()[(column, area.y)].symbol() == " "));
    }

    #[test]
    fn right_span_cluster_wins_width_and_keeps_one_cell_gap() {
        let app = AppState::test_new();
        let row = split_span_row("abcdefghij", "R9");
        let mut terminal = Terminal::new(TestBackend::new(12, 1)).unwrap();
        terminal
            .draw(|frame| {
                render_section_row(
                    frame,
                    Rect::new(0, 0, 12, 1),
                    &row,
                    BarColumns::default(),
                    &app.palette,
                )
            })
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
