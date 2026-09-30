//! Client-rendered API-fed custom sidebar sections (`sidebar.report_section`).
//!
//! The endpoint projects live section rows through the shell snapshot; which sections show,
//! their titles, row caps, and highlight tokens come from this client's `ui.sidebar.sections`.

use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};

use crate::api::schema::{SectionBar, SectionBarInnerAlign, SectionRow, SectionSpan};
use crate::app::state::Palette;
use crate::config::{CustomSidebarSectionConfig, SidebarSectionPlacement};
use crate::protocol::ClientShellSidebarSections;
use crate::ui::truncate_end;

/// Bottom sidebar row kept free for the collapse toggle (`«`).
const SIDEBAR_TOGGLE_ROWS: u16 = 1;
/// Rows kept for each of the spaces and agents regions before sections may grow.
const MIN_PRIMARY_REGION_HEIGHT: u16 = 3;

fn display_width_u16(text: &str) -> u16 {
    unicode_width::UnicodeWidthStr::width(text).min(u16::MAX as usize) as u16
}

fn section_rows<'a>(
    sections: &'a ClientShellSidebarSections,
    id: &str,
) -> Option<&'a [SectionRow]> {
    sections
        .sections
        .iter()
        .find(|(section_id, _)| section_id == id)
        .map(|(_, rows)| rows.as_slice())
        .filter(|rows| !rows.is_empty())
}

fn focused_pane_token<'a>(sections: &'a ClientShellSidebarSections, key: &str) -> Option<&'a str> {
    sections
        .focused_pane_tokens
        .iter()
        .find(|(token, _)| token == key)
        .map(|(_, value)| value.as_str())
}

/// Splits the expanded sidebar into the spaces/agents region and a bottom region sized to the
/// configured live sections. The sections region excludes the sidebar's right divider column and
/// the last row, which holds the sidebar collapse toggle.
pub(super) fn split_sidebar_sections_area(
    area: Rect,
    sections: &ClientShellSidebarSections,
    configs: &[CustomSidebarSectionConfig],
) -> (Rect, Rect) {
    split_sidebar_sections_area_with_height(area, sections, configs, None)
}

/// Same as [`split_sidebar_sections_area`], but `preferred_height` (rows the user dragged the
/// block to) replaces the content-derived height. It is still clamped so the spaces and agents
/// regions keep their minimum rows and the block keeps at least its divider, title and one row.
pub(super) fn split_sidebar_sections_area_with_height(
    area: Rect,
    sections: &ClientShellSidebarSections,
    configs: &[CustomSidebarSectionConfig],
    preferred_height: Option<u16>,
) -> (Rect, Rect) {
    let content_width = area.width.saturating_sub(1);
    let requested = configs
        .iter()
        .map(|config| {
            let marker_width = u16::from(config.highlight_token.is_some() && content_width > 0);
            configured_section_height(sections, config, content_width.saturating_sub(marker_width))
        })
        .fold(0u16, u16::saturating_add);
    let max = sections_max_height(area);
    let min = configs.iter().find_map(|config| {
        section_rows(sections, &config.id)
            // Divider + optional title header + one content row.
            .map(|_| 2u16.saturating_add(u16::from(config.title.is_some())))
    });
    let Some(min) = min.filter(|min| content_width > 0 && max >= *min) else {
        return (area, Rect::default());
    };
    let height = preferred_height.unwrap_or(requested).clamp(min, max);
    let primary = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height
            .saturating_sub(height)
            .saturating_sub(SIDEBAR_TOGGLE_ROWS),
    );
    let sections_area = Rect::new(area.x, primary.bottom(), content_width, height);
    (primary, sections_area)
}

/// Tallest the sections block may grow while spaces and agents keep their minimum rows.
pub(super) fn sections_max_height(area: Rect) -> u16 {
    area.height
        .saturating_sub(SIDEBAR_TOGGLE_ROWS)
        .saturating_sub(MIN_PRIMARY_REGION_HEIGHT.saturating_mul(2))
}

/// One-row grab handle on the sections block's top edge (the divider line the renderer draws
/// first). Empty when no sections are shown.
pub(super) fn sections_divider_rect(primary: Rect, sections_area: Rect) -> Rect {
    if sections_area.width == 0 || sections_area.height == 0 {
        return Rect::default();
    }
    Rect::new(sections_area.x, primary.bottom(), sections_area.width, 1)
}

fn configured_section_height(
    sections: &ClientShellSidebarSections,
    config: &CustomSidebarSectionConfig,
    row_width: u16,
) -> u16 {
    match config.placement {
        SidebarSectionPlacement::BelowAgents => section_rows(sections, &config.id)
            .map(|rows| {
                let (_, body) = split_header_row(config, rows);
                let max_rows = config.max_rows as usize;
                let capped = body.len() > max_rows;
                let content_rows = if capped {
                    max_rows.saturating_sub(1)
                } else {
                    body.len()
                };
                let body_height: u16 = body
                    .iter()
                    .take(content_rows)
                    .map(|row| row_height(row, row_width))
                    .fold(0u16, u16::saturating_add);
                body_height
                    .saturating_add(1)
                    .saturating_add(u16::from(config.title.is_some()))
                    .saturating_add(u16::from(capped))
            })
            .unwrap_or(0),
    }
}

/// Render height (in terminal lines) of a single section row at `row_width`.
/// Wrapping spans rows grow across multiple lines; everything else is 1.
fn row_height(row: &SectionRow, row_width: u16) -> u16 {
    match row {
        SectionRow::Spans { spans, right, wrap } => {
            if *wrap && right.is_empty() {
                spans_wrapped_height(spans, row_width)
            } else {
                1
            }
        }
        SectionRow::Bar { .. } => 1,
    }
}

/// Number of terminal lines a spans cluster occupies once wrapped at the given
/// width. Falls back to 1 for empty inputs or zero-width areas.
fn spans_wrapped_height(spans: &[SectionSpan], row_width: u16) -> u16 {
    if row_width == 0 {
        return 1;
    }
    let line = Line::from(
        spans
            .iter()
            .map(|span| Span::raw(span.text.as_str()))
            .collect::<Vec<_>>(),
    );
    Paragraph::new(vec![line])
        .wrap(Wrap { trim: false })
        .line_count(row_width)
        .clamp(1, u16::MAX as usize) as u16
}

/// A leading spans row with no visible left content and a non-empty right
/// cluster is hoisted onto the section's title row when the section has a
/// title: the cluster renders right-aligned beside the title instead of
/// occupying the first body line. Publishers use this to pin compact status
/// (e.g. a refresh countdown) into the header without spending a row.
fn split_header_row<'a>(
    config: &CustomSidebarSectionConfig,
    rows: &'a [SectionRow],
) -> (Option<&'a [SectionSpan]>, &'a [SectionRow]) {
    if config.title.is_none() {
        return (None, rows);
    }
    match rows.split_first() {
        Some((SectionRow::Spans { spans, right, .. }, body))
            if !right.is_empty() && !spans_have_content(spans) =>
        {
            (Some(right.as_slice()), body)
        }
        _ => (None, rows),
    }
}

fn spans_have_content(spans: &[SectionSpan]) -> bool {
    spans.iter().any(|span| display_width_u16(&span.text) > 0)
}

pub(super) fn render_sidebar_sections(
    buffer: &mut Buffer,
    area: Rect,
    sections: &ClientShellSidebarSections,
    configs: &[CustomSidebarSectionConfig],
    palette: &Palette,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let requested_height = configs
        .iter()
        .map(|config| {
            let marker_width = u16::from(config.highlight_token.is_some() && area.width > 0);
            let row_width = area.width.saturating_sub(marker_width);
            configured_section_height(sections, config, row_width)
        })
        .fold(0u16, u16::saturating_add);
    let height_overflow = requested_height > area.height;
    let mut remaining = area.height.saturating_sub(u16::from(height_overflow));
    let total_live_rows = configs
        .iter()
        .filter_map(|config| {
            section_rows(sections, &config.id).map(|rows| split_header_row(config, rows).1.len())
        })
        .sum::<usize>();
    let mut represented_rows = 0usize;
    let mut row_y = area.y;

    'sections: for config in configs {
        let Some(all_rows) = section_rows(sections, &config.id) else {
            continue;
        };
        let (header_right, rows) = split_header_row(config, all_rows);
        let max_rows = config.max_rows as usize;
        let capped = rows.len() > max_rows;
        let content_rows = if capped {
            max_rows.saturating_sub(1)
        } else {
            rows.len()
        };
        let marker_width = u16::from(config.highlight_token.is_some() && area.width > 0);
        let row_width = area.width.saturating_sub(marker_width);
        let bar_columns = BarColumns::for_rows(rows.iter().take(content_rows), row_width);
        let highlight_value = config
            .highlight_token
            .as_deref()
            .and_then(|key| focused_pane_token(sections, key));

        if remaining == 0 {
            break;
        }
        render_section_divider(buffer, Rect::new(area.x, row_y, area.width, 1), palette);
        row_y = row_y.saturating_add(1);
        remaining = remaining.saturating_sub(1);

        if let Some(title) = config.title.as_deref() {
            if remaining == 0 {
                break;
            }
            render_section_title(
                buffer,
                Rect::new(area.x, row_y, area.width, 1),
                title,
                header_right,
                palette,
            );
            row_y = row_y.saturating_add(1);
            remaining = remaining.saturating_sub(1);
        }

        for row in rows.iter().take(content_rows) {
            let height = row_height(row, row_width);
            if height == 0 || remaining < height {
                break 'sections;
            }
            let highlighted = match (row, highlight_value) {
                (SectionRow::Bar { bar }, Some(value)) => {
                    bar.match_values.iter().any(|candidate| candidate == value)
                }
                _ => false,
            };
            if marker_width > 0 {
                render_match_marker(
                    buffer,
                    Rect::new(area.x, row_y, marker_width, height),
                    highlighted,
                    palette,
                );
            }
            render_section_row(
                buffer,
                Rect::new(
                    area.x.saturating_add(marker_width),
                    row_y,
                    row_width,
                    height,
                ),
                row,
                bar_columns,
                palette,
            );
            represented_rows += 1;
            row_y = row_y.saturating_add(height);
            remaining = remaining.saturating_sub(height);
        }
        if capped {
            if remaining == 0 {
                break;
            }
            let hidden = rows.len().saturating_sub(content_rows);
            render_overflow_indicator(
                buffer,
                Rect::new(area.x, row_y, area.width, 1),
                hidden,
                palette,
            );
            represented_rows = represented_rows.saturating_add(hidden);
            row_y = row_y.saturating_add(1);
            remaining = remaining.saturating_sub(1);
        }
    }

    if height_overflow {
        render_overflow_indicator(
            buffer,
            Rect::new(
                area.x,
                area.y.saturating_add(area.height.saturating_sub(1)),
                area.width,
                1,
            ),
            total_live_rows.saturating_sub(represented_rows),
            palette,
        );
    }
}

fn render_section_divider(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    for x in area.x..area.x.saturating_add(area.width) {
        buffer[(x, area.y)].set_symbol("─");
        buffer[(x, area.y)].set_style(Style::default().fg(palette.surface_dim));
    }
}

fn render_overflow_indicator(buffer: &mut Buffer, area: Rect, hidden: usize, palette: &Palette) {
    (Paragraph::new(Span::styled(
        format!("… {hidden} more"),
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::DIM),
    )))
    .render(area, buffer);
}

fn render_match_marker(buffer: &mut Buffer, area: Rect, highlighted: bool, palette: &Palette) {
    if area.width == 0 {
        return;
    }
    let cell = &mut buffer[(area.x, area.y)];
    cell.set_symbol(" ");
    cell.set_style(Style::default());
    if highlighted {
        cell.set_symbol("▎");
        cell.set_style(
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        );
    }
}

fn render_section_row(
    buffer: &mut Buffer,
    area: Rect,
    row: &SectionRow,
    bar_columns: BarColumns,
    palette: &Palette,
) {
    match row {
        SectionRow::Spans { spans, right, wrap } => {
            render_spans(buffer, area, spans, right, *wrap, palette);
        }
        SectionRow::Bar { bar } => render_bar(
            buffer,
            area,
            bar.fraction,
            bar.title.as_deref(),
            bar.title_spans.as_deref(),
            bar.title_color.as_deref(),
            bar.label.as_deref(),
            bar.label_spans.as_deref(),
            bar.fill.as_deref(),
            bar.empty.as_deref(),
            bar.inner_spans.as_deref(),
            bar.inner_align,
            bar.solid,
            bar_columns,
            palette,
        ),
    }
}

/// Section title line: title left in the native header style (matching the
/// spaces/agents headers), optional hoisted status cluster right-aligned.
fn render_section_title(
    buffer: &mut Buffer,
    area: Rect,
    title: &str,
    right: Option<&[SectionSpan]>,
    palette: &Palette,
) {
    let right = right.unwrap_or_default();
    let right_width = right
        .iter()
        .map(|span| display_width_u16(&span.text))
        .fold(0u16, u16::saturating_add)
        .min(area.width);
    let gap = u16::from(right_width > 0 && area.width > right_width);
    let title_width = area.width.saturating_sub(right_width.saturating_add(gap));
    if title_width > 0 {
        (Paragraph::new(Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::BOLD),
        )))
        .render(Rect::new(area.x, area.y, title_width, 1), buffer);
    }
    if right_width > 0 {
        (Paragraph::new(Line::from(styled_spans(right, palette, palette.text)))
            .alignment(Alignment::Right))
        .render(
            Rect::new(
                area.x
                    .saturating_add(area.width.saturating_sub(right_width)),
                area.y,
                right_width,
                1,
            ),
            buffer,
        );
    }
}

fn render_spans(
    buffer: &mut Buffer,
    area: Rect,
    spans: &[SectionSpan],
    right: &[SectionSpan],
    wrap: bool,
    palette: &Palette,
) {
    let right_width = right
        .iter()
        .map(|span| display_width_u16(&span.text))
        .fold(0u16, u16::saturating_add)
        .min(area.width);
    let left_has_content = spans_have_content(spans);
    if !left_has_content && right_width == 0 {
        for x in area.x..area.x.saturating_add(area.width) {
            buffer[(x, area.y)].reset();
        }
        return;
    }
    let gap = u16::from(left_has_content && right_width > 0 && area.width > right_width);
    let left_width = area.width.saturating_sub(right_width.saturating_add(gap));

    // Wrapped mode: the left cluster flows across the reserved height when
    // there is no right cluster. Height is reserved by `row_height`.
    if wrap && right_width == 0 && left_has_content {
        (Paragraph::new(Line::from(styled_spans(spans, palette, palette.text)))
            .wrap(Wrap { trim: false }))
        .render(area, buffer);
        return;
    }
    if left_width > 0 {
        (Paragraph::new(Line::from(styled_spans(spans, palette, palette.text))))
            .render(Rect::new(area.x, area.y, left_width, 1), buffer);
    }
    if right_width > 0 {
        (Paragraph::new(Line::from(styled_spans(right, palette, palette.text)))
            .alignment(Alignment::Right))
        .render(
            Rect::new(
                area.x
                    .saturating_add(area.width.saturating_sub(right_width)),
                area.y,
                right_width,
                1,
            ),
            buffer,
        );
    }
}

fn section_span_style(span: &SectionSpan, palette: &Palette, default_color: Color) -> Style {
    let color = span
        .color
        .as_deref()
        .map(|color| section_color(Some(color), palette))
        .unwrap_or(default_color);
    let mut style = Style::default().fg(color);
    if span.bold {
        style = style.add_modifier(Modifier::BOLD);
    }
    if span.dim {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

fn styled_spans<'a>(
    spans: &'a [SectionSpan],
    palette: &Palette,
    default_color: Color,
) -> Vec<Span<'a>> {
    spans
        .iter()
        .map(|span| {
            Span::styled(
                span.text.as_str(),
                section_span_style(span, palette, default_color),
            )
        })
        .collect()
}

fn truncated_styled_spans<'a>(
    spans: &'a [SectionSpan],
    max_width: usize,
    palette: &Palette,
    default_color: Color,
) -> Vec<Span<'a>> {
    let joined = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    let truncated = truncate_end(&joined, max_width);
    if truncated == joined {
        return styled_spans(spans, palette, default_color);
    }

    let prefix = truncated.strip_suffix('…').unwrap_or(&truncated);
    let mut remaining = prefix.len();
    let mut rendered = Vec::with_capacity(spans.len().saturating_add(1));
    let mut ellipsis_style = spans
        .iter()
        .find(|span| !span.text.is_empty())
        .map(|span| section_span_style(span, palette, default_color))
        .unwrap_or_default();
    for span in spans {
        if remaining == 0 {
            break;
        }
        let byte_len = remaining.min(span.text.len());
        let text = span
            .text
            .get(..byte_len)
            .expect("joined title prefix ends on a character boundary");
        if !text.is_empty() {
            ellipsis_style = section_span_style(span, palette, default_color);
            rendered.push(Span::styled(text.to_string(), ellipsis_style));
        }
        remaining = remaining.saturating_sub(byte_len);
    }
    if truncated.ends_with('…') {
        rendered.push(Span::styled("…", ellipsis_style));
    }
    rendered
}

/// A bar row's point is the bar itself: reserve this many cells before
/// granting any width to the left title or the right label.
const MIN_BAR_CELLS: u16 = 6;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct BarColumns {
    title_width: u16,
    label_width: u16,
}

fn bar_title_width(bar: &SectionBar) -> u16 {
    match bar.title_spans.as_deref() {
        Some(spans) => spans
            .iter()
            .map(|span| display_width_u16(&span.text))
            .fold(0u16, u16::saturating_add),
        None => bar
            .title
            .as_deref()
            .map(display_width_u16)
            .unwrap_or_default(),
    }
}

fn bar_label_width(bar: &SectionBar) -> u16 {
    match bar.label_spans.as_deref() {
        Some(spans) => spans
            .iter()
            .map(|span| display_width_u16(&span.text))
            .fold(0u16, u16::saturating_add),
        None => bar
            .label
            .as_deref()
            .map(display_width_u16)
            .unwrap_or_default(),
    }
}

impl BarColumns {
    fn for_rows<'a>(rows: impl Iterator<Item = &'a SectionRow>, area_width: u16) -> Self {
        let (title_width, label_width) = rows
            .filter_map(|row| match row {
                SectionRow::Bar { bar } => Some((bar_title_width(bar), bar_label_width(bar))),
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
    buffer: &mut Buffer,
    area: Rect,
    fraction: f64,
    title: Option<&str>,
    title_spans: Option<&[SectionSpan]>,
    title_color: Option<&str>,
    label: Option<&str>,
    label_spans: Option<&[SectionSpan]>,
    fill: Option<&str>,
    empty: Option<&str>,
    inner_spans: Option<&[SectionSpan]>,
    inner_align: Option<SectionBarInnerAlign>,
    solid: bool,
    columns: BarColumns,
    palette: &Palette,
) {
    if area.width == 0 {
        return;
    }

    let bar_x = area
        .x
        .saturating_add(columns.title_width)
        .saturating_add(columns.title_gap());
    let bar_width = area
        .width
        .saturating_sub(column_footprint(columns.title_width))
        .saturating_sub(column_footprint(columns.label_width));
    let filled = ((fraction.clamp(0.0, 1.0) * f64::from(bar_width)).round() as u16).min(bar_width);

    for offset in 0..bar_width {
        let cell = &mut buffer[(bar_x + offset, area.y)];
        if solid {
            // Direct port of omp's renderUsageBar (command-controller.ts):
            // `fraction` is USED. The bar draws the FREE fraction as a
            // gradient-colored █ strip from the left; the '<pct>% free'
            // label straddles the strip edge (inverse video over the strip,
            // gradient fg past it); the remainder is dim ░.
            let free_width = bar_width - filled;
            if offset < free_width {
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
                cell.set_style(
                    Style::default()
                        .fg(palette.surface_dim)
                        .add_modifier(Modifier::DIM),
                );
            }
        } else if offset < filled {
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

    // Text rendered on top of the bar cells. In solid mode this mirrors
    // omp's renderUsageBar label handling: the label starts at
    // clamp(freeCells - labelWidth, 0, barWidth - labelWidth); the part over
    // the free strip renders inverse (status bg, dark fg), the part past
    // the strip renders with the status foreground. Other modes keep the
    // simple overlay: span text replaces cell glyphs with span styling.
    if let Some(inner_spans) = inner_spans {
        let inner_width: u16 = inner_spans
            .iter()
            .map(|span| span.text.chars().count() as u16)
            .sum();
        let mut column = match inner_align {
            Some(SectionBarInnerAlign::Right) => bar_width.saturating_sub(inner_width),
            Some(SectionBarInnerAlign::Boundary) => {
                if solid {
                    // omp: labelStart = clamp(freeCells - labelWidth,
                    // 0, barWidth - labelWidth). `filled` holds used cells,
                    // so free cells = bar_width - filled.
                    let free_cells = bar_width.saturating_sub(filled);
                    free_cells
                        .saturating_sub(inner_width)
                        .min(bar_width.saturating_sub(inner_width))
                } else if filled + inner_width > bar_width {
                    bar_width.saturating_sub(inner_width)
                } else {
                    filled
                }
            }
            _ => 0,
        };
        for span in inner_spans {
            for ch in span.text.chars() {
                if column >= bar_width {
                    break;
                }
                let cell = &mut buffer[(bar_x + column, area.y)];
                let mut style = cell.style();
                let fg = match span.color.as_deref() {
                    Some(color) => section_color(Some(color), palette),
                    None if solid => palette.surface0,
                    None => palette.text,
                };
                style = style.fg(fg);
                if solid {
                    let on_strip = column < bar_width.saturating_sub(filled);
                    let status =
                        fill.map_or_else(|| palette.accent, |f| section_color(Some(f), palette));
                    if on_strip {
                        // inverse: dark text on the status-colored strip
                        style = style.bg(status).fg(palette.surface0);
                    } else {
                        // past the strip: status-colored foreground, no bg
                        style = style.fg(status);
                    }
                }
                if span.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if span.dim {
                    style = style.add_modifier(Modifier::DIM);
                }
                cell.set_symbol(ch.to_string().as_str());
                cell.set_style(style);
                column += 1;
            }
            if column >= bar_width {
                break;
            }
        }
    }

    if columns.title_width > 0 {
        let area = Rect::new(area.x, area.y, columns.title_width, 1);
        if let Some(title_spans) = title_spans {
            (Paragraph::new(Line::from(truncated_styled_spans(
                title_spans,
                columns.title_width.into(),
                palette,
                palette.text,
            ))))
            .render(area, buffer);
        } else {
            let title = title
                .map(|title| truncate_end(title, columns.title_width.into()))
                .unwrap_or_default();
            (Paragraph::new(Span::styled(
                title,
                Style::default().fg(section_color(title_color, palette)),
            )))
            .render(area, buffer);
        }
    }
    if columns.label_width > 0 {
        let area = Rect::new(
            area.x + area.width.saturating_sub(columns.label_width),
            area.y,
            columns.label_width,
            1,
        );
        if let Some(label_spans) = label_spans {
            (Paragraph::new(Line::from(truncated_styled_spans(
                label_spans,
                columns.label_width.into(),
                palette,
                palette.subtext0,
            )))
            .alignment(Alignment::Right))
            .render(area, buffer);
        } else {
            let label = label
                .map(|label| truncate_end(label, columns.label_width.into()))
                .unwrap_or_default();
            (Paragraph::new(Span::styled(label, Style::default().fg(palette.subtext0)))
                .alignment(Alignment::Right))
            .render(area, buffer);
        }
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

    fn span(text: &str) -> SectionSpan {
        SectionSpan {
            text: text.into(),
            color: None,
            bold: false,
            dim: false,
        }
    }

    fn config(id: &str, title: Option<&str>) -> CustomSidebarSectionConfig {
        CustomSidebarSectionConfig {
            id: id.into(),
            title: title.map(Into::into),
            ..CustomSidebarSectionConfig::default()
        }
    }

    fn live(id: &str, rows: Vec<SectionRow>) -> ClientShellSidebarSections {
        ClientShellSidebarSections {
            sections: vec![(id.into(), rows)],
            focused_pane_tokens: Vec::new(),
        }
    }

    fn line(buffer: &Buffer, y: u16) -> String {
        (buffer.area.x..buffer.area.right())
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    }

    #[test]
    fn sections_region_is_reserved_only_for_configured_live_sections() {
        let area = Rect::new(0, 0, 30, 40);
        let rows = vec![SectionRow::Spans {
            spans: vec![span("ready")],
            right: Vec::new(),
            wrap: false,
        }];
        let (primary, sections) =
            split_sidebar_sections_area(area, &live("build", rows.clone()), &[]);
        assert_eq!((primary, sections), (area, Rect::default()));

        let (primary, sections) = split_sidebar_sections_area(
            area,
            &live("other", rows.clone()),
            &[config("build", Some("Build"))],
        );
        assert_eq!((primary, sections), (area, Rect::default()));

        let (primary, sections) = split_sidebar_sections_area(
            area,
            &live("build", rows),
            &[config("build", Some("Build"))],
        );
        // Divider + title + one row above the toggle row, excluding the divider column.
        assert_eq!(sections, Rect::new(0, 36, 29, 3));
        assert_eq!(primary, Rect::new(0, 0, 30, 36));
    }

    #[test]
    fn title_row_hoists_leading_right_only_cluster_and_renders_rows() {
        let rows = vec![
            SectionRow::Spans {
                spans: Vec::new(),
                right: vec![span("5m")],
                wrap: false,
            },
            SectionRow::Spans {
                spans: vec![span("ready")],
                right: vec![span("ok")],
                wrap: false,
            },
        ];
        let sections = live("build", rows);
        let configs = [config("build", Some("Build"))];
        let area = Rect::new(0, 0, 20, 3);
        let mut buffer = Buffer::empty(area);
        render_sidebar_sections(
            &mut buffer,
            area,
            &sections,
            &configs,
            &Palette::catppuccin(),
        );

        assert!(line(&buffer, 0).chars().all(|ch| ch == '─'));
        let title = line(&buffer, 1);
        assert!(title.starts_with(" Build "), "{title:?}");
        assert!(title.trim_end().ends_with("5m"), "{title:?}");
        let row = line(&buffer, 2);
        assert!(row.starts_with("ready"), "{row:?}");
        assert!(row.trim_end().ends_with("ok"), "{row:?}");
    }

    #[test]
    fn bar_rows_render_title_label_and_focus_highlight() {
        let bar = |title: &str, value: &str| SectionRow::Bar {
            bar: SectionBar {
                fraction: 0.5,
                title: Some(title.into()),
                label: Some("50%".into()),
                match_values: vec![value.into()],
                ..SectionBar::default()
            },
        };
        let sections = ClientShellSidebarSections {
            sections: vec![("usage".into(), vec![bar("a", "one"), bar("b", "two")])],
            focused_pane_tokens: vec![("broker".into(), "two".into())],
        };
        let configs = [CustomSidebarSectionConfig {
            highlight_token: Some("broker".into()),
            ..config("usage", None)
        }];
        let area = Rect::new(0, 0, 24, 3);
        let mut buffer = Buffer::empty(area);
        render_sidebar_sections(
            &mut buffer,
            area,
            &sections,
            &configs,
            &Palette::catppuccin(),
        );

        let first = line(&buffer, 1);
        let second = line(&buffer, 2);
        assert!(first.starts_with(" a"), "{first:?}");
        assert!(first.trim_end().ends_with("50%"), "{first:?}");
        assert!(first.contains('█') && first.contains('░'), "{first:?}");
        assert!(second.starts_with("▎b"), "{second:?}");
    }

    #[test]
    fn capped_sections_report_hidden_rows() {
        let rows = (0..5)
            .map(|index| SectionRow::Spans {
                spans: vec![span(&format!("row{index}"))],
                right: Vec::new(),
                wrap: false,
            })
            .collect();
        let configs = [CustomSidebarSectionConfig {
            max_rows: 3,
            ..config("build", None)
        }];
        let area = Rect::new(0, 0, 20, 4);
        let mut buffer = Buffer::empty(area);
        render_sidebar_sections(
            &mut buffer,
            area,
            &live("build", rows),
            &configs,
            &Palette::catppuccin(),
        );

        assert!(line(&buffer, 1).starts_with("row0"));
        assert!(line(&buffer, 2).starts_with("row1"));
        assert!(
            line(&buffer, 3).starts_with("… 3 more"),
            "{:?}",
            line(&buffer, 3)
        );
    }
}
