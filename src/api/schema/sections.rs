use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SidebarReportSectionParams {
    #[schemars(length(min = 1, max = 32))]
    pub section_id: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(range(min = 1, max = 86_400_000))]
    pub ttl_ms: Option<u64>,
    #[schemars(length(max = 16))]
    pub rows: Vec<SectionRow>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SectionRow {
    Spans(#[schemars(length(max = 8))] Vec<SectionSpan>),
    Bar {
        fraction: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SectionSpan {
    pub text: String,
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
    fn report_section_request_parses_external_row_variants() {
        let request: crate::api::schema::Request = serde_json::from_value(serde_json::json!({
            "id": "req",
            "method": "sidebar.report_section",
            "params": {
                "section_id": "build",
                "source": "test:build",
                "rows": [
                    {"spans": [{"text": "ready", "color": "green"}]},
                    {"bar": {"fraction": 0.5, "label": "5/10"}}
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
                SectionRow::Spans(vec![SectionSpan {
                    text: "ready".into(),
                    color: Some("green".into()),
                    bold: false,
                    dim: false,
                }]),
                SectionRow::Bar {
                    fraction: 0.5,
                    label: Some("5/10".into()),
                },
            ]
        );
    }
}
