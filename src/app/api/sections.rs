use crate::api::schema::{
    ResponseResult, SectionBar, SectionRow, SectionSpan, SidebarReportSectionParams,
};
use crate::app::App;

use super::responses::{encode_error, encode_success};

const MAX_SECTION_ROWS: usize = 24;
const MAX_SECTION_SPANS: usize = 8;
const MAX_SECTION_TEXT_CHARS: usize = 240;
const MAX_SECTION_MATCH_VALUES: usize = 8;
const MAX_SECTION_MATCH_VALUE_CHARS: usize = 200;

impl App {
    pub(super) fn handle_sidebar_report_section(
        &mut self,
        id: String,
        params: SidebarReportSectionParams,
    ) -> String {
        if !super::super::api_helpers::metadata_token_name_is_valid(&params.section_id) {
            return encode_error(
                id,
                "invalid_sidebar_section_id",
                "section_id must be 1-32 ASCII letters, digits, underscores, or hyphens",
            );
        }
        let source = match super::super::api_helpers::normalize_metadata_source(params.source) {
            Ok(source) => source,
            Err(message) => return encode_error(id, "invalid_metadata_source", message),
        };
        if let Some(owner) = self
            .state
            .sidebar_section_reports
            .owner(&params.section_id)
            .filter(|owner| *owner != source)
        {
            return encode_error(
                id,
                "sidebar_section_owned",
                format!("sidebar section is owned by source {owner}"),
            );
        }
        let ttl = match super::super::api_helpers::normalize_metadata_ttl(params.ttl_ms) {
            Ok(ttl) => ttl,
            Err(message) => return encode_error(id, "invalid_metadata_ttl", message),
        };
        let rows = match normalize_section_rows(params.rows) {
            Ok(rows) => rows,
            Err((code, message)) => return encode_error(id, code, message),
        };

        match self.state.sidebar_section_reports.report(
            params.section_id,
            &source,
            params.seq,
            ttl,
            rows,
            std::time::Instant::now(),
        ) {
            Ok(true) => self.sync_agent_metadata_deadline(),
            Ok(false) => {}
            Err(crate::app::sidebar_sections::SidebarSectionReportError::Owned { owner }) => {
                return encode_error(
                    id,
                    "sidebar_section_owned",
                    format!("sidebar section is owned by source {owner}"),
                );
            }
            Err(crate::app::sidebar_sections::SidebarSectionReportError::SequenceSourceLimit) => {
                return encode_error(
                    id,
                    "metadata_sequence_source_limit",
                    format!(
                        "sidebar section may track at most {} sequenced sources",
                        crate::metadata_tokens::MAX_SEQUENCE_SOURCES
                    ),
                );
            }
        }

        encode_success(id, ResponseResult::Ok {})
    }
}

fn normalize_section_rows(
    rows: Vec<SectionRow>,
) -> Result<Vec<SectionRow>, (&'static str, String)> {
    if rows.len() > MAX_SECTION_ROWS {
        return Err((
            "invalid_sidebar_section_rows",
            format!("a sidebar section may contain at most {MAX_SECTION_ROWS} rows"),
        ));
    }

    rows.into_iter()
        .map(|row| match row {
            SectionRow::Spans { spans, right } => Ok(SectionRow::Spans {
                spans: normalize_section_spans(spans)?,
                right: normalize_section_spans(right)?,
            }),
            SectionRow::Bar { bar } => Ok(SectionRow::Bar {
                bar: SectionBar {
                    fraction: bar.fraction.clamp(0.0, 1.0),
                    title: bar.title.as_deref().and_then(normalize_bar_layout_text),
                    title_spans: bar
                        .title_spans
                        .map(|spans| normalize_bar_spans(spans, "title"))
                        .transpose()?,
                    title_color: normalize_section_color(bar.title_color)?,
                    label: bar.label.as_deref().and_then(normalize_bar_layout_text),
                    label_spans: bar
                        .label_spans
                        .map(|spans| normalize_bar_spans(spans, "label"))
                        .transpose()?,
                    match_values: normalize_bar_match_values(bar.match_values)?,
                    fill: normalize_section_color(bar.fill)?,
                    empty: normalize_section_color(bar.empty)?,
                },
            }),
        })
        .collect()
}

/// Bar titles, labels, their styled spans, and match values are layout-bearing:
/// preserve deliberate whitespace while stripping controls and bounding input.
fn normalize_layout_text(text: &str, max_chars: usize) -> Option<String> {
    let normalized = text
        .chars()
        .filter(|ch| !ch.is_control())
        .take(max_chars)
        .collect::<String>();
    normalized
        .chars()
        .any(|ch| !ch.is_whitespace())
        .then_some(normalized)
}

fn normalize_bar_layout_text(text: &str) -> Option<String> {
    normalize_layout_text(text, MAX_SECTION_TEXT_CHARS)
}

fn normalize_bar_spans(
    spans: Vec<SectionSpan>,
    field: &'static str,
) -> Result<Vec<SectionSpan>, (&'static str, String)> {
    if spans.len() > MAX_SECTION_SPANS {
        return Err((
            "invalid_sidebar_section_spans",
            format!("a sidebar bar {field} may contain at most {MAX_SECTION_SPANS} spans"),
        ));
    }

    spans
        .into_iter()
        .map(|span| {
            Ok(SectionSpan {
                text: normalize_bar_layout_text(&span.text).unwrap_or_default(),
                color: normalize_section_color(span.color)?,
                bold: span.bold,
                dim: span.dim,
            })
        })
        .collect()
}

fn normalize_bar_match_values(values: Vec<String>) -> Result<Vec<String>, (&'static str, String)> {
    if values.len() > MAX_SECTION_MATCH_VALUES {
        return Err((
            "invalid_sidebar_section_match_values",
            format!("a sidebar bar may contain at most {MAX_SECTION_MATCH_VALUES} match values"),
        ));
    }

    Ok(values
        .into_iter()
        .filter_map(|value| normalize_layout_text(&value, MAX_SECTION_MATCH_VALUE_CHARS))
        .collect())
}

fn normalize_section_spans(
    spans: Vec<SectionSpan>,
) -> Result<Vec<SectionSpan>, (&'static str, String)> {
    if spans.len() > MAX_SECTION_SPANS {
        return Err((
            "invalid_sidebar_section_spans",
            format!("a sidebar section row may contain at most {MAX_SECTION_SPANS} spans"),
        ));
    }

    spans
        .into_iter()
        .map(|span| {
            Ok(SectionSpan {
                text: super::sanitized_notification_text(&span.text, MAX_SECTION_TEXT_CHARS)
                    .unwrap_or_default(),
                color: normalize_section_color(span.color)?,
                bold: span.bold,
                dim: span.dim,
            })
        })
        .collect()
}

fn normalize_section_color(
    color: Option<String>,
) -> Result<Option<String>, (&'static str, String)> {
    if let Some(color) = color.as_deref() {
        if !section_color_is_valid(color) {
            return Err((
                "invalid_sidebar_section_color",
                format!("unsupported sidebar section color: {color}"),
            ));
        }
    }
    Ok(color)
}

fn section_color_is_valid(color: &str) -> bool {
    matches!(
        color,
        "accent" | "text" | "subtext0" | "green" | "yellow" | "red" | "blue" | "mauve" | "peach"
    ) || color
        .strip_prefix('#')
        .is_some_and(|hex| hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ErrorResponse, Method, Request, SuccessResponse};
    use crate::config::Config;

    fn test_app() -> App {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        App::new(
            &Config::default(),
            true,
            None,
            api_rx,
            crate::api::EventHub::default(),
        )
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

    fn params(section_id: &str, rows: Vec<SectionRow>) -> SidebarReportSectionParams {
        SidebarReportSectionParams {
            section_id: section_id.into(),
            source: "test:section".into(),
            seq: None,
            ttl_ms: None,
            rows,
        }
    }

    #[test]
    fn report_section_validates_id_row_and_span_limits() {
        let mut app = test_app();
        for invalid_id in ["", "bad.name", &"x".repeat(33)] {
            let response = app.handle_sidebar_report_section(
                "req".into(),
                params(invalid_id, vec![span_row("ok")]),
            );
            let error: ErrorResponse = serde_json::from_str(&response).unwrap();
            assert_eq!(error.error.code, "invalid_sidebar_section_id");
        }

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                std::iter::repeat_n(span_row("row"), MAX_SECTION_ROWS).collect(),
            ),
        );
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert_eq!(
            app.state
                .sidebar_section_reports
                .rows("build")
                .map(<[SectionRow]>::len),
            Some(MAX_SECTION_ROWS)
        );

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                std::iter::repeat_n(span_row("row"), MAX_SECTION_ROWS + 1).collect(),
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_rows");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Spans {
                    spans: std::iter::repeat_n(
                        SectionSpan {
                            text: "span".into(),
                            color: None,
                            bold: false,
                            dim: false,
                        },
                        MAX_SECTION_SPANS + 1,
                    )
                    .collect(),
                    right: Vec::new(),
                }],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_spans");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Spans {
                    spans: Vec::new(),
                    right: std::iter::repeat_n(
                        SectionSpan {
                            text: "span".into(),
                            color: None,
                            bold: false,
                            dim: false,
                        },
                        MAX_SECTION_SPANS + 1,
                    )
                    .collect(),
                }],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_spans");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Spans {
                    spans: vec![SectionSpan {
                        text: "span".into(),
                        color: Some("cyan".into()),
                        bold: false,
                        dim: false,
                    }],
                    right: Vec::new(),
                }],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_color");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Bar {
                    bar: SectionBar {
                        fraction: 0.5,
                        title: None,
                        title_spans: None,
                        title_color: None,
                        label: None,
                        label_spans: None,
                        match_values: Vec::new(),
                        fill: Some("cyan".into()),
                        empty: None,
                    },
                }],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_color");
    }

    #[test]
    fn report_section_accepts_named_and_rgb_title_colors_and_rejects_invalid() {
        let mut app = test_app();
        for (section_id, color) in [("named", "peach"), ("rgb", "#ff9f52")] {
            let response = app.handle_sidebar_report_section(
                section_id.into(),
                params(
                    section_id,
                    vec![SectionRow::Bar {
                        bar: SectionBar {
                            fraction: 0.5,
                            title: Some("usage".into()),
                            title_spans: None,
                            title_color: Some(color.into()),
                            label: None,
                            label_spans: None,
                            match_values: Vec::new(),
                            fill: None,
                            empty: None,
                        },
                    }],
                ),
            );
            let success: SuccessResponse = serde_json::from_str(&response).unwrap();
            assert_eq!(success.result, ResponseResult::Ok {});
            let Some(SectionRow::Bar { bar }) = app
                .state
                .sidebar_section_reports
                .rows(section_id)
                .and_then(|rows| rows.first())
            else {
                panic!("stored bar row");
            };
            assert_eq!(bar.title_color.as_deref(), Some(color));
        }

        let response = app.handle_sidebar_report_section(
            "invalid".into(),
            params(
                "invalid",
                vec![SectionRow::Bar {
                    bar: SectionBar {
                        fraction: 0.5,
                        title: Some("usage".into()),
                        title_spans: None,
                        title_color: Some("cyan".into()),
                        label: None,
                        label_spans: None,
                        match_values: Vec::new(),
                        fill: None,
                        empty: None,
                    },
                }],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_color");
    }

    #[test]
    fn report_section_accepts_blank_spans_row() {
        let mut app = test_app();
        let blank = SectionRow::Spans {
            spans: Vec::new(),
            right: Vec::new(),
        };
        let response =
            app.handle_sidebar_report_section("blank".into(), params("blank", vec![blank.clone()]));
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert_eq!(
            app.state.sidebar_section_reports.rows("blank"),
            Some([blank].as_slice())
        );
    }

    #[test]
    fn bar_layout_text_preserves_padding_strips_controls_and_drops_blanks() {
        let rows = normalize_section_rows(vec![
            SectionRow::Bar {
                bar: SectionBar {
                    fraction: 0.05,
                    title: Some("  leading title".into()),
                    title_spans: Some(vec![
                        SectionSpan {
                            text: " al*\n ".into(),
                            color: Some("#ff9f52".into()),
                            bold: false,
                            dim: true,
                        },
                        SectionSpan {
                            text: "7d\t fa".into(),
                            color: Some("#ff9f52".into()),
                            bold: false,
                            dim: false,
                        },
                    ]),
                    title_color: None,
                    label: Some("  5% \u{21bb}  30m".into()),
                    label_spans: Some(vec![
                        SectionSpan {
                            text: "  5%\n".into(),
                            color: Some("subtext0".into()),
                            bold: false,
                            dim: false,
                        },
                        SectionSpan {
                            text: " \u{21bb}\t  30m".into(),
                            color: Some("#c8d0dc".into()),
                            bold: true,
                            dim: false,
                        },
                    ]),
                    match_values: vec!["http://broker-a\n".into(), "\t ".into()],
                    fill: None,
                    empty: None,
                },
            },
            SectionRow::Bar {
                bar: SectionBar {
                    fraction: 0.42,
                    title: Some("mu\nm\t 5h".into()),
                    title_spans: None,
                    title_color: None,
                    label: Some("  4\n2%\t \u{21bb} 30m".into()),
                    label_spans: None,
                    match_values: Vec::new(),
                    fill: None,
                    empty: None,
                },
            },
            SectionRow::Bar {
                bar: SectionBar {
                    fraction: 1.0,
                    title: Some(" \t\n ".into()),
                    title_spans: None,
                    title_color: None,
                    label: Some("\t \n".into()),
                    label_spans: None,
                    match_values: Vec::new(),
                    fill: None,
                    empty: None,
                },
            },
        ])
        .unwrap();

        let SectionRow::Bar { bar: padded } = &rows[0] else {
            panic!("padded bar");
        };
        assert_eq!(padded.title.as_deref(), Some("  leading title"));
        assert_eq!(padded.label.as_deref(), Some("  5% \u{21bb}  30m"));
        let title_spans = padded.title_spans.as_deref().expect("styled title");
        assert_eq!(title_spans[0].text, " al* ");
        assert_eq!(title_spans[0].color.as_deref(), Some("#ff9f52"));
        assert!(title_spans[0].dim);
        assert_eq!(title_spans[1].text, "7d fa");
        assert!(!title_spans[1].dim);
        let label_spans = padded.label_spans.as_deref().expect("styled label");
        assert_eq!(label_spans[0].text, "  5%");
        assert_eq!(label_spans[0].color.as_deref(), Some("subtext0"));
        assert_eq!(label_spans[1].text, " \u{21bb}  30m");
        assert_eq!(label_spans[1].color.as_deref(), Some("#c8d0dc"));
        assert!(label_spans[1].bold);
        assert_eq!(padded.match_values, ["http://broker-a"]);

        let SectionRow::Bar { bar: stripped } = &rows[1] else {
            panic!("control-stripped bar");
        };
        assert_eq!(stripped.title.as_deref(), Some("mum 5h"));
        assert_eq!(stripped.label.as_deref(), Some("  42% \u{21bb} 30m"));

        let SectionRow::Bar { bar: blank } = &rows[2] else {
            panic!("blank bar");
        };
        assert_eq!(blank.title, None);
        assert_eq!(blank.label, None);

        let overlong = format!("\n{}tail", "x".repeat(MAX_SECTION_TEXT_CHARS));
        assert_eq!(
            normalize_bar_layout_text(&overlong),
            Some("x".repeat(MAX_SECTION_TEXT_CHARS))
        );

        let too_many = normalize_bar_spans(
            std::iter::repeat_n(
                SectionSpan {
                    text: "x".into(),
                    color: None,
                    bold: false,
                    dim: false,
                },
                MAX_SECTION_SPANS + 1,
            )
            .collect(),
            "title_spans",
        )
        .unwrap_err();
        assert_eq!(too_many.0, "invalid_sidebar_section_spans");

        let invalid_color = normalize_bar_spans(
            vec![SectionSpan {
                text: "x".into(),
                color: Some("cyan".into()),
                bold: false,
                dim: false,
            }],
            "title_spans",
        )
        .unwrap_err();
        assert_eq!(invalid_color.0, "invalid_sidebar_section_color");

        let too_many_matches = normalize_bar_match_values(
            std::iter::repeat_n("broker".to_string(), MAX_SECTION_MATCH_VALUES + 1).collect(),
        )
        .unwrap_err();
        assert_eq!(too_many_matches.0, "invalid_sidebar_section_match_values");
    }

    #[test]
    fn report_section_sanitizes_text_clamps_bars_and_returns_ok() {
        let mut app = test_app();
        let response = app.handle_api_request(Request {
            id: "req".into(),
            method: Method::SidebarReportSection(params(
                "build",
                vec![
                    SectionRow::Spans {
                        spans: vec![SectionSpan {
                            text: "  ready\n\t now\u{7}  ".into(),
                            color: Some("accent".into()),
                            bold: true,
                            dim: false,
                        }],
                        right: vec![SectionSpan {
                            text: "  5\n min  ".into(),
                            color: Some("blue".into()),
                            bold: false,
                            dim: true,
                        }],
                    },
                    SectionRow::Bar {
                        bar: SectionBar {
                            fraction: 2.0,
                            title: Some("  mum\n 5h  ".into()),
                            title_spans: None,
                            title_color: None,
                            label: Some("  10\n jobs  ".into()),
                            label_spans: None,
                            match_values: Vec::new(),
                            fill: Some("green".into()),
                            empty: Some("#123456".into()),
                        },
                    },
                    SectionRow::Bar {
                        bar: SectionBar {
                            fraction: -1.0,
                            title: None,
                            title_spans: None,
                            title_color: None,
                            label: None,
                            label_spans: None,
                            match_values: Vec::new(),
                            fill: None,
                            empty: None,
                        },
                    },
                ],
            )),
        });
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert_eq!(
            app.state.sidebar_section_reports.rows("build"),
            Some(
                [
                    SectionRow::Spans {
                        spans: vec![SectionSpan {
                            text: "ready now".into(),
                            color: Some("accent".into()),
                            bold: true,
                            dim: false,
                        }],
                        right: vec![SectionSpan {
                            text: "5 min".into(),
                            color: Some("blue".into()),
                            bold: false,
                            dim: true,
                        }],
                    },
                    SectionRow::Bar {
                        bar: SectionBar {
                            fraction: 1.0,
                            title: Some("  mum 5h  ".into()),
                            title_spans: None,
                            title_color: None,
                            label: Some("  10 jobs  ".into()),
                            label_spans: None,
                            match_values: Vec::new(),
                            fill: Some("green".into()),
                            empty: Some("#123456".into()),
                        },
                    },
                    SectionRow::Bar {
                        bar: SectionBar {
                            fraction: 0.0,
                            title: None,
                            title_spans: None,
                            title_color: None,
                            label: None,
                            label_spans: None,
                            match_values: Vec::new(),
                            fill: None,
                            empty: None,
                        },
                    },
                ]
                .as_slice()
            )
        );
    }

    #[test]
    fn report_section_rejects_foreign_source_with_owner_error() {
        let mut app = test_app();
        let mut owned = params("build", vec![span_row("owned")]);
        owned.source = "source-a".into();
        let _: SuccessResponse =
            serde_json::from_str(&app.handle_sidebar_report_section("owner".into(), owned))
                .unwrap();

        let mut foreign = params("build", vec![span_row("foreign")]);
        foreign.source = "source-b".into();
        let response = app.handle_sidebar_report_section("foreign".into(), foreign);
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "sidebar_section_owned");
        assert!(error.error.message.contains("source-a"));
        assert_eq!(
            app.state.sidebar_section_reports.rows("build"),
            Some([span_row("owned")].as_slice())
        );
    }

    #[test]
    fn report_section_rejects_stale_sequence_without_replacing_rows() {
        let mut app = test_app();
        let mut first = params("build", vec![span_row("new")]);
        first.seq = Some(2);
        let _: SuccessResponse =
            serde_json::from_str(&app.handle_sidebar_report_section("first".into(), first))
                .unwrap();

        let mut stale = params("build", vec![span_row("stale")]);
        stale.seq = Some(1);
        let response = app.handle_sidebar_report_section("stale".into(), stale);
        let success: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(success.result, ResponseResult::Ok {});
        assert_eq!(
            app.state.sidebar_section_reports.rows("build"),
            Some([span_row("new")].as_slice())
        );
    }

    #[test]
    fn report_section_ttl_expires_through_metadata_deadline() {
        let mut app = test_app();
        let mut report = params("build", vec![span_row("temporary")]);
        report.ttl_ms = Some(1);
        let response = app.handle_sidebar_report_section("req".into(), report);
        let _: SuccessResponse = serde_json::from_str(&response).unwrap();
        let deadline = app.agent_metadata_deadline.expect("section deadline");

        app.expire_metadata_at(deadline, deadline);

        assert_eq!(app.state.sidebar_section_reports.rows("build"), None);
        assert_eq!(app.agent_metadata_deadline, None);
    }
}
