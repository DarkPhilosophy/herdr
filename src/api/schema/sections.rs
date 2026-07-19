use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SidebarReportSectionParams {
    #[schemars(length(min = 1, max = 32))]
    pub section_id: String,
    /// Stable reporter identity. The first accepted non-empty report owns the
    /// section until that owner clears it or its content expires.
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 86_400_000))]
    pub ttl_ms: Option<u64>,
    /// Rows to display. An empty list from the owning source clears the
    /// section and releases ownership for another source.
    #[schemars(length(max = 24))]
    pub rows: Vec<SectionRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum SectionRow {
    Spans {
        #[schemars(length(max = 8))]
        spans: Vec<SectionSpan>,
        /// Optional cluster rendered right-aligned after at least one cell of
        /// separation from the left cluster.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        #[schemars(length(max = 8))]
        right: Vec<SectionSpan>,
    },
    Bar {
        bar: SectionBar,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SectionBar {
    pub fraction: f64,
    /// Left-aligned title rendered before the bar cells.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Styled left-aligned title fragments. When present, these override
    /// `title` and `title_color`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 8))]
    pub title_spans: Option<Vec<SectionSpan>>,
    /// Color for the title. Uses the span color grammar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Styled right-aligned label fragments. When present, these override
    /// `label`. The joined fragments are measured and truncated as one unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(max = 8))]
    pub label_spans: Option<Vec<SectionSpan>>,
    /// Focus-matching values for this row. When its section config names a
    /// pane metadata token, the row is marked while the focused pane's token
    /// value equals any entry. At most 8 values of 200 characters each.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = 8))]
    pub match_values: Vec<String>,
    /// Uniform color for filled cells. Uses the span color grammar; when
    /// absent, filled cells use the default positional gradient.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// Color for remainder cells. Uses the span color grammar; when absent,
    /// remainder cells use the default dim style.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SectionSpan {
    pub text: String,
    /// Named theme token or `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "super::is_false")]
    pub dim: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_section_request_parses_span_clusters_and_bar_rows() {
        let request: crate::api::schema::Request = serde_json::from_value(serde_json::json!({
            "id": "req",
            "method": "sidebar.report_section",
            "params": {
                "section_id": "build",
                "source": "test:build",
                "rows": [
                    {
                        "spans": [{"text": "ready", "color": "green"}],
                        "right": [{"text": "5m", "dim": true}]
                    },
                    {"bar": {
                        "fraction": 0.5,
                        "title": "queue",
                        "title_spans": [
                            {"text": "qu* ", "color": "peach", "dim": true},
                            {"text": "5h", "color": "peach"}
                        ],
                        "title_color": "peach",
                        "label": "5/10",
                        "label_spans": [
                            {"text": "5/10 "},
                            {"text": "↻ 30m", "color": "#c8d0dc", "bold": true}
                        ],
                        "match_values": ["http://broker-a", "http://broker-b"],
                        "fill": "#123456",
                        "empty": "subtext0"
                    }}
                ]
            }
        }))
        .unwrap();
        let crate::api::schema::Method::SidebarReportSection(params) = request.method else {
            panic!("wrong method");
        };

        assert_eq!(
            params.rows,
            vec![
                SectionRow::Spans {
                    spans: vec![SectionSpan {
                        text: "ready".into(),
                        color: Some("green".into()),
                        bold: false,
                        dim: false,
                    }],
                    right: vec![SectionSpan {
                        text: "5m".into(),
                        color: None,
                        bold: false,
                        dim: true,
                    }],
                },
                SectionRow::Bar {
                    bar: SectionBar {
                        fraction: 0.5,
                        title: Some("queue".into()),
                        title_spans: Some(vec![
                            SectionSpan {
                                text: "qu* ".into(),
                                color: Some("peach".into()),
                                bold: false,
                                dim: true,
                            },
                            SectionSpan {
                                text: "5h".into(),
                                color: Some("peach".into()),
                                bold: false,
                                dim: false,
                            },
                        ]),
                        title_color: Some("peach".into()),
                        label: Some("5/10".into()),
                        label_spans: Some(vec![
                            SectionSpan {
                                text: "5/10 ".into(),
                                color: None,
                                bold: false,
                                dim: false,
                            },
                            SectionSpan {
                                text: "↻ 30m".into(),
                                color: Some("#c8d0dc".into()),
                                bold: true,
                                dim: false,
                            },
                        ]),
                        match_values: vec!["http://broker-a".into(), "http://broker-b".into(),],
                        fill: Some("#123456".into()),
                        empty: Some("subtext0".into()),
                    },
                },
            ]
        );
    }

    #[test]
    fn report_section_request_parses_blank_spans_row_without_right_cluster() {
        let request: crate::api::schema::Request = serde_json::from_value(serde_json::json!({
            "id": "req",
            "method": "sidebar.report_section",
            "params": {
                "section_id": "usage",
                "source": "test:usage",
                "rows": [{"spans": []}]
            }
        }))
        .unwrap();
        let crate::api::schema::Method::SidebarReportSection(params) = request.method else {
            panic!("wrong method");
        };

        assert_eq!(
            params.rows,
            vec![SectionRow::Spans {
                spans: Vec::new(),
                right: Vec::new(),
            }]
        );
    }
}
