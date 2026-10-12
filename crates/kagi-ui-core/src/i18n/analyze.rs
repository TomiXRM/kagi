//! Analyze metric scales and bounded-presentation wording (ADR-0048).
use super::{lang, Lang};
use kagi_domain::hotspot::FileMetric;

pub fn risk_label(risk: f64) -> String {
    match lang() {
        Lang::En => format!("risk {risk:.2} of 1.0"),
        Lang::Ja => format!("risk {risk:.2} / 1.0"),
    }
}
pub fn metric_description(file: &FileMetric) -> String {
    format!(
        "{} — {}, {} LOC, {} commits",
        file.path,
        risk_label(file.risk),
        file.loc,
        file.commits
    )
}
pub fn scale_legend(map: bool) -> &'static str {
    match (lang(), map) {
        (Lang::En, true) => "Risk 0.0–1.0 · color: absolute risk · size: LOC (zero LOC: commits)",
        (Lang::En, false) => "Risk 0.0–1.0 · bars: relative to highest risk",
        (Lang::Ja, true) => "Risk 0.0–1.0 · 色: 絶対値 · 面積: LOC (LOC が 0: commits)",
        (Lang::Ja, false) => "Risk 0.0–1.0 · 棒: 最大 risk に対する比率",
    }
}
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut result = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i != 0 && (digits.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(digit);
    }
    result
}
pub fn truncation(shown: usize, total: usize) -> String {
    let (shown, total) = (grouped(shown), grouped(total));
    match lang() {
        Lang::En => format!("Showing {shown} of {total} — narrow the period or filter"),
        Lang::Ja => format!("{total} 件中 {shown} 件を表示 — 期間または filter を絞ってください"),
    }
}
