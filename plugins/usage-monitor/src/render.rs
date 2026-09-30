//! Pure row rendering. No I/O, so every function is unit-testable.

use serde_json::{json, Value};

const COLOR_STALE: &str = "#6c7086";
const COLOR_PROVIDER: &str = "#f9e2af";

/// omp `resolveUsageGradientRgb`: red -> orange -> yellow -> green, driven by
/// the REMAINING fraction.
pub fn gradient_rgb(remaining_fraction: f64) -> String {
    let c = remaining_fraction.clamp(0.0, 1.0);
    let (start, end, progress) = if c <= 0.5 {
        ((255.0, 69.0, 58.0), (255.0, 159.0, 10.0), c * 2.0)
    } else if c <= 0.75 {
        ((255.0, 159.0, 10.0), (255.0, 214.0, 10.0), (c - 0.5) * 4.0)
    } else {
        ((255.0, 214.0, 10.0), (48.0, 209.0, 88.0), (c - 0.75) * 4.0)
    };
    let lerp = |a: f64, b: f64| (a + (b - a) * progress).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        lerp(start.0, end.0),
        lerp(start.1, end.1),
        lerp(start.2, end.2)
    )
}

pub fn format_countdown(seconds: i64) -> String {
    let s = seconds.max(0);
    let (days, rem) = (s / 86_400, s % 86_400);
    let (hours, rem) = (rem / 3_600, rem % 3_600);
    let minutes = rem / 60;
    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes:02}m")
    } else {
        format!("{minutes}m")
    }
}

/// omp `buildRedactionMap` form: three-char anchor + `***`.
pub fn mask_identity(value: &str) -> String {
    let v = value.trim().to_lowercase();
    let anchor: String = v.chars().take(3).collect();
    format!("{anchor}***")
}

pub struct Usage<'a> {
    pub account: &'a str,
    pub provider: &'a str,
    pub window: &'a str,
    /// Used fraction, 0.0..=1.0.
    pub used_fraction: f64,
    /// Seconds until reset, when known.
    pub resets_in: Option<i64>,
    /// Age of the underlying snapshot in seconds when it is no longer live.
    /// `Some` renders the row dimmed with an age suffix.
    pub stale_age: Option<i64>,
}

/// Two rows: identity line with countdown, then a solid free-space bar.
pub fn usage_rows(u: &Usage) -> [Value; 2] {
    let used = u.used_fraction.clamp(0.0, 1.0);
    let free = 1.0 - used;
    let free_pct = (free * 100.0).round() as i64;
    let dim = u.stale_age.is_some();
    let right = match u.resets_in {
        Some(s) => json!([{ "text": format_countdown(s), "dim": true }]),
        None => json!([]),
    };
    let mut identity = vec![
        json!({ "text": format!("{} ", u.account), "dim": dim }),
        json!({ "text": format!("({}) ", u.provider), "color": COLOR_PROVIDER, "dim": dim }),
        json!({ "text": u.window, "dim": true }),
    ];
    if let Some(age) = u.stale_age {
        identity.push(json!({ "text": format!(" {} old", format_countdown(age)), "dim": true }));
    }
    let fill = if dim { COLOR_STALE.to_owned() } else { gradient_rgb(free) };
    [
        json!({ "spans": identity, "right": right }),
        json!({
            "bar": {
                "fraction": used,
                "solid": true,
                "inner_align": "boundary",
                "inner_spans": [{ "text": format!("{free_pct}% free"), "bold": !dim, "dim": dim }],
                "fill": fill,
            }
        }),
    ]
}

/// Explicit placeholder so stale or missing data is never drawn as a live bar.
pub fn note_row(text: &str) -> Value {
    json!({ "spans": [{ "text": text, "dim": true }], "right": [] })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_endpoints() {
        assert_eq!(gradient_rgb(0.0), "#ff453a");
        assert_eq!(gradient_rgb(1.0), "#30d158");
        assert_eq!(gradient_rgb(-3.0), gradient_rgb(0.0));
        assert_eq!(gradient_rgb(9.0), gradient_rgb(1.0));
    }

    #[test]
    fn countdown_units() {
        assert_eq!(format_countdown(-5), "0m");
        assert_eq!(format_countdown(59 * 60), "59m");
        assert_eq!(format_countdown(2 * 3600 + 6 * 60), "2h06m");
        assert_eq!(format_countdown(3 * 86_400 + 4 * 3600), "3d4h");
    }

    #[test]
    fn masking_is_utf8_safe_and_short_safe() {
        assert_eq!(mask_identity(" Ual@x.com "), "ual***");
        assert_eq!(mask_identity("ab"), "ab***");
        assert_eq!(mask_identity("éèàü"), "éèà***");
    }

    #[test]
    fn free_percent_is_complement_of_used() {
        let rows = usage_rows(&Usage {
            account: "ual***",
            provider: "Anthropic",
            window: "5h",
            used_fraction: 0.08,
            resets_in: Some(7_560),
            stale_age: None,
        });
        assert_eq!(rows[1]["bar"]["inner_spans"][0]["text"], "92% free");
        assert_eq!(rows[0]["right"][0]["text"], "2h06m");
    }

    #[test]
    fn out_of_range_usage_is_clamped() {
        let rows = usage_rows(&Usage {
            account: "a",
            provider: "P",
            window: "w",
            used_fraction: 7.0,
            resets_in: None,
            stale_age: None,
        });
        assert_eq!(rows[1]["bar"]["inner_spans"][0]["text"], "0% free");
        assert_eq!(rows[0]["right"].as_array().map(Vec::len), Some(0));
    }
}
