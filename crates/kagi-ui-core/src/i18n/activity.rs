//! Activity bucket descriptions. Git domain words stay English in Japanese.
use super::{lang, Lang};

pub fn bucket(date: &str, commits: u32, merges: u32, authors: u32) -> String {
    bucket_for(lang(), date, commits, merges, authors)
}
fn bucket_for(lang: Lang, date: &str, commits: u32, merges: u32, authors: u32) -> String {
    match lang {
        Lang::En => format!(
            "{date}: {commits} commit{}, {merges} merge{}, {authors} author{}",
            if commits == 1 { "" } else { "s" },
            if merges == 1 { "" } else { "s" },
            if authors == 1 { "" } else { "s" }
        ),
        Lang::Ja => {
            format!("{date}: {commits} 件の commit、{merges} 件の merge、{authors} 人の作成者")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_bucket_counts_are_localized_with_singular_and_zero_plural() {
        assert_eq!(
            bucket_for(Lang::En, "date", 1, 1, 1),
            "date: 1 commit, 1 merge, 1 author"
        );
        assert_eq!(
            bucket_for(Lang::En, "date", 0, 0, 0),
            "date: 0 commits, 0 merges, 0 authors"
        );
        assert_eq!(
            bucket_for(Lang::En, "date", 24, 2, 3),
            "date: 24 commits, 2 merges, 3 authors"
        );
        assert_eq!(
            bucket_for(Lang::Ja, "date", 0, 1, 24),
            "date: 0 件の commit、1 件の merge、24 人の作成者"
        );
    }
}
