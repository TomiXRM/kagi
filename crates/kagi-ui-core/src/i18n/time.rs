//! Relative-age wording; count/unit calculation stays in `crate::time`.
use super::Lang;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgeUnit {
    Minute,
    Hour,
    Day,
    Month,
    Year,
}

#[derive(Clone, Copy)]
pub enum AgeStyle {
    Compact,
    Short,
    Description,
}

pub fn relative_age(language: Lang, parts: Option<(i64, AgeUnit)>, style: AgeStyle) -> String {
    let Some((count, unit)) = parts else {
        return match (language, style) {
            (Lang::Ja, _) => "たった今",
            (Lang::En, AgeStyle::Short) => "now",
            (Lang::En, _) => "just now",
        }
        .into();
    };
    let (abbreviation, english, japanese) = match unit {
        AgeUnit::Minute => ("m", "minute", "分"),
        AgeUnit::Hour => ("h", "hour", "時間"),
        AgeUnit::Day => ("d", "day", "日"),
        AgeUnit::Month => ("mo", "month", "か月"),
        AgeUnit::Year => ("y", "year", "年"),
    };
    match (language, style) {
        (Lang::Ja, _) => format!("{count}{japanese}前"),
        (Lang::En, AgeStyle::Short) => format!("{count}{abbreviation}"),
        (Lang::En, AgeStyle::Compact) => format!("{count}{abbreviation} ago"),
        (Lang::En, AgeStyle::Description) => {
            let plural = if count == 1 { "" } else { "s" };
            format!("{count} {english}{plural} ago")
        }
    }
}
