use crate::api::schema::{ResponseResult, SectionRow, SectionSpan, SidebarReportSectionParams};
use crate::app::App;

use super::responses::{encode_error, encode_success};

const MAX_SECTION_ROWS: usize = 16;
const MAX_SECTION_SPANS: usize = 8;
const MAX_SECTION_TEXT_CHARS: usize = 240;

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
            Err(()) => {
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
            SectionRow::Spans(spans) => normalize_section_spans(spans).map(SectionRow::Spans),
            SectionRow::Bar { fraction, label } => Ok(SectionRow::Bar {
                fraction: fraction.clamp(0.0, 1.0),
                label: label.as_deref().and_then(|label| {
                    super::sanitized_notification_text(label, MAX_SECTION_TEXT_CHARS)
                }),
            }),
        })
        .collect()
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
            if let Some(color) = span.color.as_deref() {
                if !section_color_is_valid(color) {
                    return Err((
                        "invalid_sidebar_section_color",
                        format!("unsupported sidebar section color: {color}"),
                    ));
                }
            }
            Ok(SectionSpan {
                text: super::sanitized_notification_text(&span.text, MAX_SECTION_TEXT_CHARS)
                    .unwrap_or_default(),
                color: span.color,
                bold: span.bold,
                dim: span.dim,
            })
        })
        .collect()
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
        SectionRow::Spans(vec![SectionSpan {
            text: text.into(),
            color: None,
            bold: false,
            dim: false,
        }])
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
                std::iter::repeat_n(span_row("row"), MAX_SECTION_ROWS + 1).collect(),
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_rows");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Spans(
                    std::iter::repeat_n(
                        SectionSpan {
                            text: "span".into(),
                            color: None,
                            bold: false,
                            dim: false,
                        },
                        MAX_SECTION_SPANS + 1,
                    )
                    .collect(),
                )],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_spans");

        let response = app.handle_sidebar_report_section(
            "req".into(),
            params(
                "build",
                vec![SectionRow::Spans(vec![SectionSpan {
                    text: "span".into(),
                    color: Some("cyan".into()),
                    bold: false,
                    dim: false,
                }])],
            ),
        );
        let error: ErrorResponse = serde_json::from_str(&response).unwrap();
        assert_eq!(error.error.code, "invalid_sidebar_section_color");
    }

    #[test]
    fn report_section_sanitizes_text_clamps_bars_and_returns_ok() {
        let mut app = test_app();
        let response = app.handle_api_request(Request {
            id: "req".into(),
            method: Method::SidebarReportSection(params(
                "build",
                vec![
                    SectionRow::Spans(vec![SectionSpan {
                        text: "  ready\n\t now\u{7}  ".into(),
                        color: Some("accent".into()),
                        bold: true,
                        dim: false,
                    }]),
                    SectionRow::Bar {
                        fraction: 2.0,
                        label: Some("  10\n jobs  ".into()),
                    },
                    SectionRow::Bar {
                        fraction: -1.0,
                        label: None,
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
                    SectionRow::Spans(vec![SectionSpan {
                        text: "ready now".into(),
                        color: Some("accent".into()),
                        bold: true,
                        dim: false,
                    }]),
                    SectionRow::Bar {
                        fraction: 1.0,
                        label: Some("10 jobs".into()),
                    },
                    SectionRow::Bar {
                        fraction: 0.0,
                        label: None,
                    },
                ]
                .as_slice()
            )
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
