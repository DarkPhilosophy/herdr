//! Runtime state for API-fed custom sidebar sections.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::schema::SectionRow;

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarSections {
    entries: HashMap<String, SidebarSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SidebarSectionReportError {
    Owned { owner: String },
    SequenceSourceLimit,
}

#[derive(Debug, Clone, Default)]
struct SidebarSection {
    owner: Option<String>,
    rows: Vec<SectionRow>,
    sequences: HashMap<String, u64>,
    expires_at: Option<Instant>,
}

impl SidebarSections {
    pub(crate) fn report(
        &mut self,
        section_id: String,
        source: &str,
        seq: Option<u64>,
        ttl: Option<Duration>,
        rows: Vec<SectionRow>,
        now: Instant,
    ) -> Result<bool, SidebarSectionReportError> {
        let section = self.entries.entry(section_id).or_default();
        if let Some(owner) = section.owner.as_deref() {
            if owner != source {
                return Err(SidebarSectionReportError::Owned {
                    owner: owner.to_string(),
                });
            }
        } else if rows.is_empty() {
            return Ok(false);
        }

        if !crate::metadata_tokens::accept_sequence(&mut section.sequences, source, seq)
            .map_err(|()| SidebarSectionReportError::SequenceSourceLimit)?
        {
            return Ok(false);
        }

        if rows.is_empty() {
            section.owner = None;
            section.expires_at = None;
            section.rows.clear();
        } else {
            section.owner.get_or_insert_with(|| source.to_string());
            section.expires_at = ttl.map(|ttl| now + ttl);
            section.rows = rows;
        }
        Ok(true)
    }

    pub(crate) fn owner(&self, section_id: &str) -> Option<&str> {
        self.entries
            .get(section_id)
            .and_then(|section| section.owner.as_deref())
    }

    pub(crate) fn rows(&self, section_id: &str) -> Option<&[SectionRow]> {
        self.entries
            .get(section_id)
            .map(|section| section.rows.as_slice())
            .filter(|rows| !rows.is_empty())
    }

    pub(crate) fn next_expiry(&self) -> Option<Instant> {
        self.entries
            .values()
            .filter(|section| !section.rows.is_empty())
            .filter_map(|section| section.expires_at)
            .min()
    }

    pub(crate) fn expire_at(&mut self, now: Instant) -> bool {
        let mut changed = false;
        for section in self.entries.values_mut() {
            if section.expires_at.is_some_and(|deadline| deadline <= now) {
                changed |= !section.rows.is_empty();
                section.rows.clear();
                section.expires_at = None;
                section.owner = None;
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{SectionRow, SectionSpan};

    fn row(text: &str) -> SectionRow {
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

    #[test]
    fn sequence_history_survives_expiry_while_new_owner_starts_fresh() {
        let now = Instant::now();
        let mut sections = SidebarSections::default();
        assert_eq!(
            sections.report(
                "build".into(),
                "source-a",
                Some(2),
                Some(Duration::from_millis(1)),
                vec![row("new")],
                now,
            ),
            Ok(true)
        );
        assert!(sections.expire_at(now + Duration::from_millis(1)));
        assert_eq!(sections.rows("build"), None);
        assert_eq!(
            sections.report(
                "build".into(),
                "source-a",
                Some(1),
                None,
                vec![row("stale")],
                now + Duration::from_millis(2),
            ),
            Ok(false)
        );
        assert_eq!(
            sections.report(
                "build".into(),
                "source-b",
                Some(1),
                None,
                vec![row("rebound")],
                now + Duration::from_millis(2),
            ),
            Ok(true)
        );
        assert_eq!(sections.rows("build"), Some([row("rebound")].as_slice()));
        assert_eq!(
            sections.report(
                "deploy".into(),
                "source-a",
                Some(1),
                None,
                vec![row("scoped")],
                now,
            ),
            Ok(true)
        );
    }

    #[test]
    fn foreign_source_is_rejected_until_owner_clears_and_releases() {
        let now = Instant::now();
        let mut sections = SidebarSections::default();
        assert_eq!(
            sections.report(
                "build".into(),
                "source-a",
                Some(1),
                None,
                vec![row("owned")],
                now,
            ),
            Ok(true)
        );
        assert_eq!(
            sections.report(
                "build".into(),
                "source-b",
                Some(1),
                None,
                vec![row("foreign")],
                now,
            ),
            Err(SidebarSectionReportError::Owned {
                owner: "source-a".into(),
            })
        );
        assert_eq!(sections.rows("build"), Some([row("owned")].as_slice()));
        assert_eq!(
            sections.report("build".into(), "source-a", Some(2), None, Vec::new(), now,),
            Ok(true)
        );
        assert_eq!(sections.rows("build"), None);
        assert_eq!(
            sections.report(
                "build".into(),
                "source-b",
                Some(1),
                None,
                vec![row("handoff")],
                now,
            ),
            Ok(true)
        );
        assert_eq!(sections.rows("build"), Some([row("handoff")].as_slice()));
    }

    #[test]
    fn sequenced_source_limit_matches_metadata_tokens() {
        let mut sections = SidebarSections::default();
        let now = Instant::now();
        for index in 0..crate::metadata_tokens::MAX_SEQUENCE_SOURCES {
            let source = format!("source-{index}");
            assert_eq!(
                sections.report("build".into(), &source, Some(1), None, vec![row("ok")], now,),
                Ok(true)
            );
            assert_eq!(
                sections.report("build".into(), &source, Some(2), None, Vec::new(), now,),
                Ok(true)
            );
        }
        assert_eq!(
            sections.report(
                "build".into(),
                "one-too-many",
                Some(1),
                None,
                vec![row("rejected")],
                now,
            ),
            Err(SidebarSectionReportError::SequenceSourceLimit)
        );
    }
}
