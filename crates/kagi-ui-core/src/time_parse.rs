//! Timestamp parsing for `git log`'s strict-ISO8601 output, moved here from
//! `kagi-ui-file-history` (which re-exports it) so the shared commit-row
//! model can derive relative dates. Pure: no deps beyond `std`.

/// Parse an ISO-8601 / RFC-3339 timestamp (`git --date=iso-strict`, e.g.
/// `2026-01-02T15:04:05+09:00`) into seconds since the Unix epoch.
///
/// A tiny hand-rolled parser — the project has no chrono dependency in the UI
/// layer, and the format is fixed by our own `git log` invocation.  Returns
/// `None` on any malformed input so callers can fall back gracefully.
pub fn iso_to_epoch(s: &str) -> Option<i64> {
    let s = s.trim();
    // Expect at least "YYYY-MM-DDTHH:MM:SS".
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;

    // Days from the civil date (Howard Hinnant's algorithm), giving days since
    // 1970-01-01.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // [0, 11], Mar=0
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    let days = era * 146_097 + doe - 719_468;

    let mut secs = days * 86_400 + hour * 3_600 + min * 60 + sec;

    // Timezone offset suffix: 'Z' (UTC) or ±HH:MM / ±HHMM.
    if let Some(off) = s.get(19..) {
        let off = off.trim();
        if !off.is_empty() && off != "Z" && off != "z" {
            let sign = match off.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => 0,
            };
            if sign != 0 {
                let rest = &off[1..];
                let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
                if digits.len() >= 4 {
                    let oh: i64 = digits[0..2].parse().ok()?;
                    let om: i64 = digits[2..4].parse().ok()?;
                    secs -= sign * (oh * 3_600 + om * 60);
                } else if digits.len() >= 2 {
                    let oh: i64 = digits[0..2].parse().ok()?;
                    secs -= sign * oh * 3_600;
                }
            }
        }
    }

    Some(secs)
}
