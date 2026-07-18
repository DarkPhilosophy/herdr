//! Runtime state for API-fed custom sidebar sections.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::api::schema::SectionRow;

#[derive(Debug, Clone, Default)]
pub(crate) struct SidebarSections {
    entries: HashMap<String, SidebarSection>,
}

#[derive(Debug, Clone, Default)]
struct SidebarSection {
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
    ) -> Result<bool, ()> {
        let section = self.entries.entry(section_id).or_default();
        if !crate::metadata_tokens::accept_sequence(&mut section.sequences, source, seq)? {
            return Ok(false);
        }

        section.expires_at = if rows.is_empty() {
            None
        } else {
            ttl.map(|ttl| now + ttl)
        };
        section.rows = rows;
        Ok(true)
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
        SectionRow::Spans(vec![SectionSpan {
            text: text.into(),
            color: None,
            bold: false,
            dim: false,
        }])
    }

    #[test]
    fn sequence_state_survives_expiry_and_is_scoped_per_section() {
        let now = Instant::now();
        let mut sections = SidebarSections::default();
        assert_eq!(
            sections.report(
                "build".into(),
                "test",
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
                "test",
                Some(1),
                None,
                vec![row("stale")],
                now + Duration::from_millis(2),
            ),
            Ok(false)
        );
        assert_eq!(sections.rows("build"), None);
        assert_eq!(
            sections.report(
                "deploy".into(),
                "test",
                Some(1),
                None,
                vec![row("accepted")],
                now,
            ),
            Ok(true)
        );
    }

    #[test]
    fn sequenced_source_limit_matches_metadata_tokens() {
        let mut sections = SidebarSections::default();
        let now = Instant::now();
        for index in 0..crate::metadata_tokens::MAX_SEQUENCE_SOURCES {
            assert_eq!(
                sections.report(
                    "build".into(),
                    &format!("source-{index}"),
                    Some(1),
                    None,
                    vec![row("ok")],
                    now,
                ),
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
            Err(())
        );
    }
}
