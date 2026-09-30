mod config;
mod render;
mod source;

use config::Config;
use render::{mask_identity, note_row, usage_rows, Usage};
use serde_json::{json, Value};
use source::Snapshot;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SOURCE_ID: &str = "plugin:herdr-usage-monitor";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn label_for(cfg: &Config, provider: &str) -> String {
    cfg.labels
        .get(provider)
        .cloned()
        .unwrap_or_else(|| config::derive_label(provider))
}

/// `anthropic:7d:fable` -> `("7d", Some("fable"))`; `openai-codex:primary` -> `("primary", None)`.
fn split_limit(limit_id: &str) -> (&str, Option<&str>) {
    let rest = limit_id.split_once(':').map_or(limit_id, |(_, r)| r);
    match rest.split_once(':') {
        Some((head, extra)) => (head, Some(extra)),
        None => (rest, None),
    }
}

/// Heads that name a time window themselves (`5h`, `7d`) or defer to the
/// window label (`primary`, `secondary`).
fn is_time_head(head: &str) -> bool {
    let h = head.to_lowercase();
    matches!(h.as_str(), "primary" | "secondary")
        || (h.len() > 1
            && h.ends_with(['h', 'd', 'w', 'm'])
            && h[..h.len() - 1].chars().all(|c| c.is_ascii_digit()))
}

fn window_rank(head: &str, window_label: &str) -> u8 {
    let probe = if is_time_head(head) && !matches!(head, "primary" | "secondary") {
        head
    } else {
        window_label
    };
    let p = probe.trim().to_lowercase();
    if p.contains("hour") || p.ends_with('h') || p.contains("daily") || p == "primary" {
        0
    } else if p.contains("day") || p.ends_with('d') || p.contains("week") {
        1
    } else {
        2
    }
}

fn window_tag(s: &Snapshot) -> String {
    let (head, extra) = split_limit(&s.limit_id);
    let span = s.window_label.as_deref().unwrap_or("").trim();
    let lower = span.to_lowercase();
    if matches!(head, "primary" | "secondary") {
        return match lower.as_str() {
            "5 hours" | "5 hour" => "5h".to_owned(),
            "7 days" | "7 day" => "7d".to_owned(),
            "" => head.to_owned(),
            _ => lower,
        };
    }
    if is_time_head(head) {
        return match extra {
            Some(e) => format!("{head} {}", config::derive_label(e)),
            None => head.to_owned(),
        };
    }
    // Non-time heads (`quota`, `credits`, `usd`, ...): the DB's own window
    // label is the readable form (`Weekly Quota`, `Plan Period`).
    if !span.is_empty() {
        return span.to_owned();
    }
    match extra {
        Some(e) => format!("{head} {}", config::derive_label(e)),
        None => head.to_owned(),
    }
}

/// Builds every row for one publish cycle. Pure given a connection and clock.
pub fn compose(conn: &rusqlite::Connection, cfg: &Config, now_ms: i64) -> Result<Vec<Value>, source::SourceError> {
    // Load through the grace window; rows older than `max_age` render dimmed.
    let load_window = cfg.max_age_ms.saturating_add(cfg.grace_ms);
    let snaps = source::fresh_snapshots(conn, now_ms, load_window)?;

    let mut by_provider: BTreeMap<String, Vec<Snapshot>> = BTreeMap::new();
    for s in snaps {
        if cfg.providers.is_empty() || cfg.providers.contains(&s.provider) {
            by_provider.entry(s.provider.clone()).or_default().push(s);
        }
    }

    // Configured order wins; otherwise alphabetical (BTreeMap order).
    let order: Vec<String> = if cfg.providers.is_empty() {
        by_provider.keys().cloned().collect()
    } else {
        cfg.providers.clone()
    };

    let mut rows = Vec::new();
    for provider in order {
        let label = label_for(cfg, &provider);
        let Some(mut list) = by_provider.remove(&provider) else {
            // Only surface a placeholder for explicitly requested providers.
            if !cfg.providers.is_empty() {
                let text = match source::newest_recorded_ms(conn, &provider)? {
                    Some(ms) => format!(
                        "{label} usage stale ({} old)",
                        render::format_countdown((now_ms - ms) / 1000)
                    ),
                    None => format!("{label} usage unavailable"),
                };
                rows.push(note_row(&text));
            }
            continue;
        };
        list.sort_by(|a, b| {
            let (ah, _) = split_limit(&a.limit_id);
            let (bh, _) = split_limit(&b.limit_id);
            (
                &a.account_key,
                window_rank(ah, a.window_label.as_deref().unwrap_or("")),
                &a.limit_id,
            )
                .cmp(&(
                    &b.account_key,
                    window_rank(bh, b.window_label.as_deref().unwrap_or("")),
                    &b.limit_id,
                ))
        });
        for s in &list {
            let account = match s.email.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
                Some(e) => mask_identity(e),
                None => label.to_lowercase(),
            };
            let tag = window_tag(s);
            let age_ms = now_ms - s.recorded_at_ms;
            rows.extend(usage_rows(&Usage {
                account: &account,
                provider: &label,
                window: &tag,
                used_fraction: s.used_fraction,
                resets_in: s.resets_at_ms.map(|r| (r - now_ms) / 1000),
                stale_age: (age_ms > cfg.max_age_ms).then_some(age_ms / 1000),
            }));
        }
    }
    Ok(rows)
}

fn payload(cfg: &Config, rows: Vec<Value>) -> Value {
    let rows = if rows.is_empty() { vec![note_row("no fresh omp usage data")] } else { rows };
    json!({
        "section_id": cfg.section_id,
        "source": SOURCE_ID,
        "ttl_ms": cfg.poll_secs.saturating_mul(3000),
        "rows": rows,
    })
}

fn collect(cfg: &Config) -> Result<Value, String> {
    let conn = source::open(&cfg.db_path).map_err(|e| e.to_string())?;
    let rows = compose(&conn, cfg, now_ms()).map_err(|e| e.to_string())?;
    Ok(payload(cfg, rows))
}

fn herdr_bin() -> PathBuf {
    std::env::var_os("HERDR_BIN_PATH").map(PathBuf::from).unwrap_or_else(|| "herdr".into())
}

fn publish(cfg: &Config, body: &Value) -> Result<(), String> {
    let mut child = Command::new(herdr_bin())
        .args(["sidebar", "report-section", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("spawn herdr: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(body.to_string().as_bytes()).map_err(|e| format!("write herdr stdin: {e}"))?;
    }
    let out = child.wait_with_output().map_err(|e| format!("wait herdr: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "herdr report-section ({}) failed: {}",
            cfg.section_id,
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

fn state_dir() -> PathBuf {
    std::env::var_os("HERDR_PLUGIN_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
}

fn log_line(msg: &str) {
    let path = state_dir().join("herdr-usage-monitor.log");
    // Keep the log bounded: truncate once it grows past 256 KiB.
    if std::fs::metadata(&path).map(|m| m.len() > 256 * 1024).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{} {msg}", now_ms() / 1000);
    }
}

fn error_payload(cfg: &Config, err: &str) -> Value {
    payload(cfg, vec![note_row(&format!("usage monitor error: {err}"))])
}

fn pid_path() -> PathBuf {
    state_dir().join("herdr-usage-monitor.pid")
}

fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

fn read_pid() -> Option<u32> {
    std::fs::read_to_string(pid_path()).ok()?.trim().parse().ok()
}

fn cmd_daemon(cfg: &Config) -> ExitCode {
    let _ = std::fs::write(pid_path(), std::process::id().to_string());
    let mut last_err: Option<String> = None;
    loop {
        let result = collect(cfg).and_then(|body| publish(cfg, &body).map(|()| body));
        match result {
            Ok(_) => last_err = None,
            Err(e) => {
                // Log every distinct failure once and keep it visible in the sidebar.
                if last_err.as_deref() != Some(e.as_str()) {
                    log_line(&e);
                    last_err = Some(e.clone());
                }
                let _ = publish(cfg, &error_payload(cfg, &e));
            }
        }
        std::thread::sleep(Duration::from_secs(cfg.poll_secs));
    }
}

fn cmd_start() -> ExitCode {
    if read_pid().is_some_and(pid_alive) {
        return ExitCode::SUCCESS;
    }
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("herdr-usage-monitor: cannot resolve own path: {e}");
            return ExitCode::FAILURE;
        }
    };
    match Command::new(exe)
        .arg("--daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => {
            let _ = std::fs::write(pid_path(), child.id().to_string());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("herdr-usage-monitor: cannot start daemon: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_stop(cfg: &Config) -> ExitCode {
    if let Some(pid) = read_pid().filter(|p| pid_alive(*p) && *p != std::process::id()) {
        let _ = Command::new("kill").arg(pid.to_string()).status();
    }
    let _ = std::fs::remove_file(pid_path());
    // Release the section so it disappears from the sidebar.
    let empty = json!({ "section_id": cfg.section_id, "source": SOURCE_ID, "rows": [] });
    match publish(cfg, &empty) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("herdr-usage-monitor: {e}");
            ExitCode::FAILURE
        }
    }
}

fn plugin_config_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("USAGE_MONITOR_CONFIG") {
        return Some(PathBuf::from(p));
    }
    std::env::var_os("HERDR_PLUGIN_CONFIG_DIR").map(|d| PathBuf::from(d).join("config.toml"))
}

fn main() -> ExitCode {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "--start".to_owned());
    if mode == "--start" {
        return cmd_start();
    }
    let cfg = match config::load(plugin_config_path().as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("herdr-usage-monitor: config error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match mode.as_str() {
        "--daemon" => cmd_daemon(&cfg),
        "--stop" => cmd_stop(&cfg),
        "--once" => match collect(&cfg) {
            Ok(body) => {
                println!("{body}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("herdr-usage-monitor: {e}");
                ExitCode::FAILURE
            }
        },
        "--refresh" => match collect(&cfg).and_then(|b| publish(&cfg, &b)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("herdr-usage-monitor: {e}");
                ExitCode::FAILURE
            }
        },
        other => {
            eprintln!("usage: herdr-usage-monitor [--start|--stop|--daemon|--once|--refresh] (got {other})");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::tests::{insert, memory_db};

    const NOW: i64 = 1_790_739_394_162;

    fn cfg(providers: &[&str]) -> Config {
        Config {
            db_path: PathBuf::new(),
            poll_secs: 60,
            max_age_ms: 30 * 60 * 1000,
            grace_ms: 6 * 60 * 60 * 1000,
            section_id: "usage".into(),
            providers: providers.iter().map(|s| (*s).to_owned()).collect(),
            labels: BTreeMap::from([("anthropic".to_owned(), "Claude".to_owned())]),
        }
    }

    fn texts(rows: &[Value]) -> Vec<String> {
        let mut out = Vec::new();
        for r in rows {
            for k in ["spans", "right"] {
                if let Some(a) = r[k].as_array() {
                    out.extend(a.iter().filter_map(|s| s["text"].as_str().map(str::to_owned)));
                }
            }
            if let Some(a) = r["bar"]["inner_spans"].as_array() {
                out.extend(a.iter().filter_map(|s| s["text"].as_str().map(str::to_owned)));
            }
        }
        out
    }

    #[test]
    fn providers_are_discovered_without_configuration() {
        let c = memory_db();
        insert(&c, NOW - 1, "anthropic", "ual@x", "anthropic:5h", Some(0.08), Some(NOW + 7_560_000));
        insert(&c, NOW - 1, "kimi-code", "k@x", "kimi-code:daily", Some(0.5), None);
        let t = texts(&compose(&c, &cfg(&[]), NOW).unwrap());
        assert!(t.iter().any(|s| s == "(Claude) "));
        assert!(t.iter().any(|s| s == "(Kimi Code) "));
        assert!(t.iter().any(|s| s == "92% free"));
        assert!(t.iter().any(|s| s == "2h06m"));
    }

    #[test]
    fn identities_are_masked_never_raw() {
        let c = memory_db();
        insert(&c, NOW - 1, "anthropic", "ualexa@example.com", "anthropic:5h", Some(0.1), None);
        let all = texts(&compose(&c, &cfg(&[]), NOW).unwrap()).join("|");
        assert!(all.contains("ual***"));
        assert!(!all.contains("example.com"));
    }

    #[test]
    fn beyond_grace_shows_age_note_not_a_bar() {
        let c = memory_db();
        insert(&c, NOW - 7 * 3_600_000, "anthropic", "a", "anthropic:5h", Some(0.1), None);
        let rows = compose(&c, &cfg(&["anthropic"]), NOW).unwrap();
        assert_eq!(texts(&rows), ["Claude usage stale (7h00m old)"]);
        assert!(rows.iter().all(|r| r.get("bar").is_none()));
    }

    #[test]
    fn within_grace_keeps_last_bar_dimmed_with_age() {
        let c = memory_db();
        insert(&c, NOW - 3 * 3_600_000, "anthropic", "ual@x", "anthropic:5h", Some(0.1), None);
        let rows = compose(&c, &cfg(&[]), NOW).unwrap();
        let t = texts(&rows);
        assert!(t.iter().any(|s| s == " 3h00m old"));
        assert!(t.iter().any(|s| s == "90% free"));
        assert_eq!(rows[1]["bar"]["fill"], "#6c7086");
        assert_eq!(rows[1]["bar"]["inner_spans"][0]["dim"], true);
    }

    #[test]
    fn live_row_is_not_dimmed_and_has_no_age_suffix() {
        let c = memory_db();
        insert(&c, NOW - 60_000, "anthropic", "ual@x", "anthropic:5h", Some(0.1), None);
        let rows = compose(&c, &cfg(&[]), NOW).unwrap();
        assert!(!texts(&rows).iter().any(|s| s.ends_with(" old")));
        assert_ne!(rows[1]["bar"]["fill"], "#6c7086");
    }

    #[test]
    fn missing_requested_provider_is_explicit() {
        let c = memory_db();
        let rows = compose(&c, &cfg(&["anthropic"]), NOW).unwrap();
        assert_eq!(texts(&rows), ["Claude usage unavailable"]);
    }

    #[test]
    fn provider_filter_excludes_others_and_keeps_configured_order() {
        let c = memory_db();
        insert(&c, NOW - 1, "anthropic", "a", "anthropic:5h", Some(0.1), None);
        insert(&c, NOW - 1, "zai", "z", "zai:5h", Some(0.2), None);
        insert(&c, NOW - 1, "cursor", "c", "cursor:m", Some(0.3), None);
        let t = texts(&compose(&c, &cfg(&["zai", "anthropic"]), NOW).unwrap());
        let z = t.iter().position(|s| s == "(Zai) ").unwrap();
        let a = t.iter().position(|s| s == "(Claude) ").unwrap();
        assert!(z < a);
        assert!(!t.iter().any(|s| s.contains("Cursor")));
    }

    #[test]
    fn codex_primary_secondary_use_window_label_and_sort_short_first() {
        let c = memory_db();
        for (limit, win) in [("openai-codex:secondary", "7 days"), ("openai-codex:primary", "5 hours")] {
            c.execute(
                "INSERT INTO usage_history (recorded_at, provider, account_key, email, limit_id, label, window_label, used_fraction)
                 VALUES (?1,'openai-codex','a','a@x',?2,?2,?3,0.5)",
                (NOW - 1, limit, win),
            )
            .unwrap();
        }
        let t = texts(&compose(&c, &cfg(&[]), NOW).unwrap());
        let h5 = t.iter().position(|s| s == "5h").unwrap();
        let d7 = t.iter().position(|s| s == "7d").unwrap();
        assert!(h5 < d7);
    }

    fn insert_win(c: &rusqlite::Connection, limit: &str, window: &str, used: f64, resets: Option<i64>) {
        c.execute(
            "INSERT INTO usage_history (recorded_at, provider, account_key, email, limit_id, label, window_label, used_fraction, resets_at)
             VALUES (?1,'devin','d','d@x',?2,?2,?3,?4,?5)",
            (NOW - 1, limit, window, used, resets),
        )
        .unwrap();
    }

    #[test]
    fn non_time_limit_ids_use_the_database_window_label() {
        let c = memory_db();
        insert_win(&c, "devin:quota:weekly", "Weekly Quota", 0.34, Some(NOW + 86_400_000));
        insert_win(&c, "devin:credits:flow", "Plan Period", 0.0, None);
        let t = texts(&compose(&c, &cfg(&["devin"]), NOW).unwrap());
        assert!(t.iter().any(|s| s == "Weekly Quota"));
        assert!(t.iter().any(|s| s == "Plan Period"));
        assert!(t.iter().any(|s| s == "(Devin) "));
        assert!(!t.iter().any(|s| s.contains("quota Weekly")));
    }

    #[test]
    fn any_provider_can_be_added_from_config_alone() {
        let c = memory_db();
        insert_win(&c, "devin:quota:weekly", "Weekly Quota", 0.5, None);
        assert!(compose(&c, &cfg(&["anthropic"]), NOW).unwrap().len() == 1);
        let with_devin = texts(&compose(&c, &cfg(&["anthropic", "devin"]), NOW).unwrap());
        assert!(with_devin.iter().any(|s| s == "50% free"));
    }

    #[test]
    fn time_head_detection() {
        assert!(is_time_head("5h") && is_time_head("7d") && is_time_head("30d"));
        assert!(is_time_head("primary") && is_time_head("secondary"));
        assert!(!is_time_head("quota") && !is_time_head("credits") && !is_time_head("d"));
    }

    #[test]
    fn extra_limit_suffix_becomes_readable_tag() {
        let c = memory_db();
        insert(&c, NOW - 1, "anthropic", "a", "anthropic:7d:fable", Some(0.0), None);
        let t = texts(&compose(&c, &cfg(&[]), NOW).unwrap());
        assert!(t.iter().any(|s| s == "7d Fable"));
    }

    #[test]
    fn empty_result_publishes_explicit_note() {
        let body = payload(&cfg(&[]), Vec::new());
        assert_eq!(body["rows"][0]["spans"][0]["text"], "no fresh omp usage data");
        assert_eq!(body["ttl_ms"], 180_000);
    }
}
