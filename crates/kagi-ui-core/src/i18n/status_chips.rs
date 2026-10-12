//! Informational footer chip descriptions; compact symbols stay unchanged.
use super::{lang, Lang};

#[derive(Clone, Copy)]
pub enum Chip {
    Dirty,
    Staged(usize),
    Unstaged(usize),
    Conflict(usize),
    Stash(usize),
    AheadBehind(Option<(usize, usize)>),
    NoUpstream,
    Upstream,
    Fetch { age: i64, stale: bool },
}

pub fn description(chip: Chip, upstream: &str) -> String {
    description_for(lang(), chip, upstream)
}

fn description_for(language: Lang, chip: Chip, upstream: &str) -> String {
    match (language, chip) {
        (Lang::En, Chip::Dirty) => "Uncommitted changes".into(),
        (Lang::Ja, Chip::Dirty) => "未 commit の変更があります".into(),
        (Lang::En, Chip::Staged(n)) => format!("{n} staged file{}", plural(n)),
        (Lang::Ja, Chip::Staged(n)) => format!("stage 済みのファイル {n} 件"),
        (Lang::En, Chip::Unstaged(n)) => format!("{n} unstaged file{}", plural(n)),
        (Lang::Ja, Chip::Unstaged(n)) => format!("未 stage のファイル {n} 件"),
        (Lang::En, Chip::Conflict(n)) => format!("{n} conflicted file{}", plural(n)),
        (Lang::Ja, Chip::Conflict(n)) => format!("競合ファイル {n} 件"),
        (Lang::En, Chip::Stash(n)) => format!("{n} stash entr{}", if n == 1 { "y" } else { "ies" }),
        (Lang::Ja, Chip::Stash(n)) => format!("stash {n} 件"),
        (Lang::En, Chip::AheadBehind(Some((a, b)))) => {
            format!("{a} commit{} ahead, {b} behind {upstream}", plural(a))
        }
        (Lang::Ja, Chip::AheadBehind(Some((a, b)))) => {
            format!("{upstream} より {a} commit 先行、{b} commit 遅れ")
        }
        (Lang::En, Chip::AheadBehind(None)) => {
            format!("Ahead/behind counts unavailable for {upstream}")
        }
        (Lang::Ja, Chip::AheadBehind(None)) => {
            format!("{upstream} との先行・遅れの commit 数は未取得です")
        }
        (Lang::En, Chip::NoUpstream) => "No upstream".into(),
        (Lang::Ja, Chip::NoUpstream) => "upstream がありません".into(),
        (Lang::En, Chip::Upstream) => format!("Upstream: {upstream}"),
        (Lang::Ja, Chip::Upstream) => format!("upstream: {upstream}"),
        (language, Chip::Fetch { age, stale }) => fetch_description(language, age, stale),
    }
}

fn plural(n: usize) -> &'static str {
    if n == 1 {
        ""
    } else {
        "s"
    }
}

fn fetch_description(language: Lang, age: i64, stale: bool) -> String {
    let age = age.max(0);
    let (n, en, ja) = match age {
        s if s < 60 => (s, "second", "秒"),
        s if s < 3600 => (s / 60, "minute", "分"),
        s if s < 86400 => (s / 3600, "hour", "時間"),
        s => (s / 86400, "day", "日"),
    };
    match (language, stale) {
        (Lang::En, false) => format!("Fetched {n} {en}{} ago", plural(n as usize)),
        (Lang::En, true) => format!("Last fetch {n} {en}{} ago — stale", plural(n as usize)),
        (Lang::Ja, false) => format!("{n}{ja}前に fetch 済み"),
        (Lang::Ja, true) => format!("最終 fetch は {n}{ja}前 — 古い情報です"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_chip_descriptions_are_localized() {
        assert_eq!(
            description_for(Lang::En, Chip::AheadBehind(Some((2, 1))), "origin/main"),
            "2 commits ahead, 1 behind origin/main"
        );
        assert_eq!(
            description_for(Lang::Ja, Chip::NoUpstream, ""),
            "upstream がありません"
        );
        assert_eq!(
            fetch_description(Lang::En, 720, false),
            "Fetched 12 minutes ago"
        );
        assert_eq!(
            fetch_description(Lang::En, 10800, true),
            "Last fetch 3 hours ago — stale"
        );
        assert_eq!(
            fetch_description(Lang::Ja, 10800, true),
            "最終 fetch は 3時間前 — 古い情報です"
        );
        assert_eq!(
            fetch_description(Lang::En, 1, false),
            "Fetched 1 second ago"
        );
    }
}
