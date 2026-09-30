//! Read-only access to omp's `usage_history`.
//!
//! omp stores `recorded_at` and `resets_at` in epoch MILLISECONDS. The table
//! is append-only, so a valid-looking last row can be arbitrarily old; every
//! query is gated on freshness and on the limit not having already reset.

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub provider: String,
    pub account_key: String,
    pub email: Option<String>,
    pub limit_id: String,
    pub window_label: Option<String>,
    pub used_fraction: f64,
    pub resets_at_ms: Option<i64>,
    pub recorded_at_ms: i64,
}

#[derive(Debug)]
pub enum SourceError {
    Missing,
    Sqlite(rusqlite::Error),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SourceError::Missing => write!(f, "omp database not found"),
            SourceError::Sqlite(e) => write!(f, "omp database error: {e}"),
        }
    }
}

impl From<rusqlite::Error> for SourceError {
    fn from(e: rusqlite::Error) -> Self {
        SourceError::Sqlite(e)
    }
}

/// Opens strictly read-only; never creates a file and never writes.
pub fn open(path: &Path) -> Result<Connection, SourceError> {
    if !path.exists() {
        return Err(SourceError::Missing);
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(std::time::Duration::from_secs(2))?;
    Ok(conn)
}

/// Newest row per (provider, account, limit) that is fresh and not yet reset.
pub fn fresh_snapshots(
    conn: &Connection,
    now_ms: i64,
    max_age_ms: i64,
) -> Result<Vec<Snapshot>, SourceError> {
    let mut stmt = conn.prepare(
        "SELECT provider, account_key, email, limit_id, window_label,
                used_fraction, resets_at, recorded_at
         FROM usage_history h
         WHERE recorded_at >= ?1
           AND used_fraction IS NOT NULL
           AND id = (SELECT MAX(id) FROM usage_history
                     WHERE provider = h.provider
                       AND account_key = h.account_key
                       AND limit_id = h.limit_id)
           AND (resets_at IS NULL OR resets_at = 0 OR resets_at > ?2)",
    )?;
    let rows = stmt.query_map((now_ms - max_age_ms, now_ms), |r| {
        Ok(Snapshot {
            provider: r.get(0)?,
            account_key: r.get(1)?,
            email: r.get(2)?,
            limit_id: r.get(3)?,
            window_label: r.get(4)?,
            used_fraction: r.get(5)?,
            resets_at_ms: r.get::<_, Option<i64>>(6)?.filter(|v| *v > 0),
            recorded_at_ms: r.get(7)?,
        })
    })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Newest recorded timestamp for a provider, regardless of freshness. Used to
/// say "stale (Nh old)" instead of silently showing nothing.
pub fn newest_recorded_ms(conn: &Connection, provider: &str) -> Result<Option<i64>, SourceError> {
    conn.query_row(
        "SELECT MAX(recorded_at) FROM usage_history WHERE provider = ?1",
        [provider],
        |r| r.get::<_, Option<i64>>(0),
    )
    .map_err(Into::into)
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn memory_db() -> Connection {
        let c = Connection::open_in_memory().expect("in-memory db");
        c.execute_batch(
            "CREATE TABLE usage_history (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                recorded_at INTEGER NOT NULL,
                provider TEXT NOT NULL,
                account_key TEXT NOT NULL,
                email TEXT,
                account_id TEXT,
                limit_id TEXT NOT NULL,
                label TEXT NOT NULL,
                window_label TEXT,
                used_fraction REAL,
                status TEXT,
                resets_at INTEGER
            );",
        )
        .expect("schema");
        c
    }

    pub fn insert(
        c: &Connection,
        recorded: i64,
        provider: &str,
        account: &str,
        limit: &str,
        used: Option<f64>,
        resets: Option<i64>,
    ) {
        c.execute(
            "INSERT INTO usage_history
             (recorded_at, provider, account_key, email, limit_id, label, window_label, used_fraction, resets_at)
             VALUES (?1, ?2, ?3, ?3, ?4, ?4, '5 Hour', ?5, ?6)",
            (recorded, provider, account, limit, used, resets),
        )
        .expect("insert");
    }

    const NOW: i64 = 1_790_739_394_162;
    const AGE: i64 = 30 * 60 * 1000;

    #[test]
    fn only_newest_row_per_limit_is_returned() {
        let c = memory_db();
        insert(&c, NOW - 5_000, "anthropic", "a", "anthropic:5h", Some(0.9), Some(NOW + 60_000));
        insert(&c, NOW - 1_000, "anthropic", "a", "anthropic:5h", Some(0.1), Some(NOW + 60_000));
        let got = fresh_snapshots(&c, NOW, AGE).unwrap();
        assert_eq!(got.len(), 1);
        assert!((got[0].used_fraction - 0.1).abs() < f64::EPSILON);
    }

    #[test]
    fn stale_rows_are_dropped() {
        let c = memory_db();
        insert(&c, NOW - AGE - 1, "anthropic", "a", "anthropic:5h", Some(0.1), None);
        assert!(fresh_snapshots(&c, NOW, AGE).unwrap().is_empty());
        assert_eq!(newest_recorded_ms(&c, "anthropic").unwrap(), Some(NOW - AGE - 1));
    }

    #[test]
    fn already_reset_limits_are_dropped_but_unknown_reset_is_kept() {
        let c = memory_db();
        insert(&c, NOW - 1, "p", "a", "p:5h", Some(1.0), Some(NOW - 1));
        insert(&c, NOW - 1, "p", "b", "p:5h", Some(0.2), None);
        insert(&c, NOW - 1, "p", "c", "p:5h", Some(0.3), Some(0));
        let got = fresh_snapshots(&c, NOW, AGE).unwrap();
        let mut accts: Vec<_> = got.iter().map(|s| s.account_key.as_str()).collect();
        accts.sort_unstable();
        assert_eq!(accts, ["b", "c"]);
    }

    #[test]
    fn null_usage_is_ignored() {
        let c = memory_db();
        insert(&c, NOW - 1, "p", "a", "p:m", None, None);
        assert!(fresh_snapshots(&c, NOW, AGE).unwrap().is_empty());
    }

    #[test]
    fn missing_database_is_a_distinct_error() {
        let err = open(Path::new("/nonexistent/omp/agent.db")).unwrap_err();
        assert!(matches!(err, SourceError::Missing));
    }
}
