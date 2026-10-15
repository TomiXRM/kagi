//! Display-only translations of legacy GitHub diagnostics and receipt text.
//! Never use these strings when recording an operation or logging its transport.
use crate::i18n::{lang, Lang};

pub(super) fn diagnostic(text: &str) -> Option<String> {
    diagnostic_for(text, lang())
}

fn diagnostic_for(text: &str, language: Lang) -> Option<String> {
    let say = |en: &str, ja: &str| match language {
        Lang::En => en.to_owned(),
        Lang::Ja => ja.to_owned(),
    };
    let fixed = match text {
        "pull request page repository/state mismatch"
        | "empty unfinished pull request page"
        | "pull request page cursor did not advance"
        | "empty pull request page cursor"
        | "pull request continuation requires a frozen repository"
        | "gh json: empty pull request page claims another page"
        | "gh json: expected pageInfo.endCursor" => Some((
            "Couldn’t load more pull requests. Refresh the list.",
            "Pull Request を追加で読み込めませんでした。一覧を更新してください。",
        )),
        // These formats are shared by the PR and Issue readers.
        "gh json: expected pageInfo.hasNextPage"
        | "gh json: hasNextPage without a usable endCursor"
        | "gh json: pagination cursor did not advance" => Some((
            "Couldn’t load more GitHub items. Refresh the list.",
            "GitHub の項目を追加で読み込めませんでした。一覧を更新してください。",
        )),
        "empty repository identity"
        | "missing GitHub repository identity"
        | "unexpected repo view output" => Some((
            "Couldn’t identify this repository on GitHub. Refresh the list.",
            "この repository を GitHub 上で確認できませんでした。一覧を更新してください。",
        )),
        "gh json: expected repository pull request nodes" => Some((
            "GitHub returned incomplete pull request data. Try refreshing.",
            "GitHub から必要な Pull Request 情報を取得できませんでした。更新してください。",
        )),
        "gh json: expected pull request pageInfo" => Some((
            "Couldn’t load the pull request list. Try refreshing.",
            "Pull Request 一覧を読み込めませんでした。更新してください。",
        )),
        "gh json: invalid pull request node" => Some((
            "GitHub returned unreadable pull request data.",
            "GitHub の Pull Request 情報を読み取れませんでした。",
        )),
        "gh json: pull request belongs to a different repository" => Some((
            "GitHub returned pull requests for a different repository. Refresh the list.",
            "別の repository の Pull Request が返されました。一覧を更新してください。",
        )),
        "gh json: expected repository issue nodes" => Some((
            "GitHub returned incomplete issue data. Try refreshing.",
            "GitHub から必要な Issue 情報を取得できませんでした。更新してください。",
        )),
        "gh json: expected mention search nodes" => Some((
            "Couldn’t load issues mentioning you. Try refreshing.",
            "あなたへのメンションがある Issue を読み込めませんでした。更新してください。",
        )),
        "gh json: expected issue pageInfo" => Some((
            "Couldn’t load the issue list. Try refreshing.",
            "Issue 一覧を読み込めませんでした。更新してください。",
        )),
        "gh json: missing issue number or comment ID" => Some((
            "GitHub returned incomplete issue details. Try refreshing.",
            "GitHub から必要な Issue の詳細を取得できませんでした。更新してください。",
        )),
        "gh repo list: expected a JSON array" => Some((
            "GitHub returned an unreadable repository list.",
            "GitHub の repository 一覧を読み取れませんでした。",
        )),
        "the read stopped unexpectedly" => Some((
            "Couldn’t load this organization’s repositories. Try refreshing.",
            "この組織の repository を読み込めませんでした。更新してください。",
        )),
        "gh search: expected a JSON array" => Some((
            "GitHub returned an unreadable list. Try refreshing.",
            "GitHub の一覧を読み取れませんでした。更新してください。",
        )),
        "gh label list: expected a JSON array" => Some((
            "GitHub returned an unreadable label list.",
            "GitHub のラベル一覧を読み取れませんでした。",
        )),
        "gh pr view: no pull request" => Some((
            "Couldn’t find this pull request on GitHub.",
            "GitHub 上でこの Pull Request が見つかりませんでした。",
        )),
        "gh could not name the repository to write to" => Some((
            "Couldn’t identify the destination repository on GitHub.",
            "操作先の GitHub repository を確認できませんでした。",
        )),
        "gh graphql: partial response" => Some((
            "GitHub returned incomplete data. Try refreshing.",
            "GitHub から必要な情報を取得できませんでした。更新してください。",
        )),
        "issue write has no frozen GitHub repository identity" => Some((
            "Couldn’t identify where to post. Refresh the issue list and try again.",
            "投稿先を確認できませんでした。Issue 一覧を更新して再試行してください。",
        )),
        "merge plan has blockers, no repository identity, or no frozen local deletion approval" => {
            Some((
                "This merge can’t proceed. Review the merge options and try again.",
                "Merge を実行できません。Merge の設定を確認して再試行してください。",
            ))
        }
        "issue-create unconfirmed" => {
            Some(("Issue creation unconfirmed", "Issue の作成を確認できません"))
        }
        "issue-comment unconfirmed" => Some((
            "Comment posting unconfirmed",
            "コメントの投稿を確認できません",
        )),
        _ => None,
    };
    if let Some((en, ja)) = fixed {
        return Some(say(en, ja));
    }
    for prefix in [
        "unavailable: ",
        "auth: ",
        "network: ",
        "invalid: ",
        "rate-limited: ",
        "not-found: ",
        "unknown: ",
    ] {
        if let Some(reason) = text.strip_prefix(prefix) {
            let reason = diagnostic_for(reason, language).unwrap_or_else(|| reason.to_owned());
            return Some(say(
                &format!("Couldn’t load GitHub data. {reason}"),
                &format!("GitHub の情報を読み込めませんでした。{reason}"),
            ));
        }
    }
    if let Some(repo) = text
        .strip_prefix("invalid repository identity: ")
        .or_else(|| {
            text.strip_prefix('\'')
                .and_then(|s| s.strip_suffix("' is not an <owner>/<repo> identity"))
        })
    {
        return Some(say(
            &format!("Couldn’t identify the GitHub repository: {repo}"),
            &format!("GitHub repository を確認できませんでした: {repo}"),
        ));
    }
    if let Some((actual, expected)) = text
        .strip_prefix("issue page came from ")
        .and_then(|s| s.split_once(" instead of "))
    {
        return Some(say(&format!("GitHub returned issues for {actual}, not {expected}. Refresh the list."), &format!("GitHub から {expected} ではなく {actual} の Issue が返されました。一覧を更新してください。")));
    }
    for (prefix, en, ja) in [
        (
            "gh repo list json: ",
            "Couldn’t read the repository list from GitHub.",
            "GitHub の repository 一覧を読み取れませんでした。",
        ),
        (
            "gh search json: ",
            "Couldn’t read your pull requests or issues from GitHub.",
            "GitHub の Pull Request または Issue を読み取れませんでした。",
        ),
        (
            "gh label list json: ",
            "Couldn’t read this repository’s labels.",
            "この repository のラベルを読み取れませんでした。",
        ),
        (
            "gh json: ",
            "Couldn’t read the data from GitHub. Try refreshing.",
            "GitHub の情報を読み取れませんでした。更新してください。",
        ),
    ] {
        if text.starts_with(prefix) {
            return Some(say(en, ja));
        }
    }
    for (prefix, en, ja) in [
        (
            "gh graphql: ",
            "Couldn’t load GitHub data",
            "GitHub の情報を読み込めませんでした",
        ),
        (
            "gh: ",
            "Couldn’t connect to GitHub",
            "GitHub に接続できませんでした",
        ),
    ] {
        if let Some(reason) = text.strip_prefix(prefix) {
            return Some(say(&format!("{en}: {reason}"), &format!("{ja}: {reason}")));
        }
    }
    if let Some((state, reason)) = text.split_once("; gh reported an error: ") {
        let reason = diagnostic_for(reason, language).unwrap_or_else(|| reason.to_owned());
        let message = say(
            &format!("Merge completed, but an additional step failed: {reason}"),
            &format!("Merge は完了しましたが、追加の処理に失敗しました: {reason}"),
        );
        if let Some((_, head)) = state
            .rsplit_once(" (head ")
            .filter(|(_, head)| head.ends_with(')'))
        {
            return Some(format!("{message} (HEAD {})", &head[..head.len() - 1]));
        }
        if state == "merge is confirmed" {
            return Some(message);
        }
    }
    if text.starts_with("merge not confirmed by `gh pr view --json mergedAt` (transport: ")
        && text.ends_with("); do not retry")
    {
        return Some(say("Couldn’t confirm whether the pull request was merged. Check GitHub before trying again.", "Pull Request が Merge されたか確認できません。再試行する前に GitHub を確認してください。"));
    }
    if text.starts_with("gh pr comment #") || text.ends_with("; the comment may or may not have been posted — it was not re-read, so do not retry blindly") {
        return Some(say("Couldn’t confirm whether your comment was posted. Check GitHub before posting again.", "コメントが投稿されたか確認できません。再投稿する前に GitHub を確認してください。"));
    }
    if text.starts_with("gh issue write ") || text.ends_with("; the issue write may or may not have been posted — it was not re-read, so do not retry blindly") {
        return Some(say("Couldn’t confirm whether your issue or comment was posted. Check GitHub before posting again.", "Issue またはコメントが投稿されたか確認できません。再投稿する前に GitHub を確認してください。"));
    }
    if text.starts_with("gh pr edit #")
        || (text.contains("; the reviewers, assignees and labels on #")
            && text.ends_with(
                "may or may not have changed — they were not re-read, so do not retry blindly",
            ))
    {
        let number = pr_number(text)?;
        return Some(say(&format!("Couldn’t confirm the reviewers, assignees or labels on #{number}. Check GitHub before trying again."), &format!("#{number} のレビュアー・担当者・ラベルの変更を確認できません。再試行する前に GitHub を確認してください。")));
    }
    if text.starts_with("gh pr review ")
        || (text.contains(" review on #")
            && text.ends_with(
                "may or may not have been submitted — it was not re-read, so do not retry blindly",
            ))
    {
        let number = pr_number(text)?;
        return Some(say(&format!("Couldn’t confirm whether your review on #{number} was submitted. Check GitHub before submitting again."), &format!("#{number} のレビューが送信されたか確認できません。再送信する前に GitHub を確認してください。")));
    }
    if let Some(rest) = text.strip_prefix('#') {
        let (number, state) = rest.split_once(' ')?;
        if !number.is_empty() && number.bytes().all(|c| c.is_ascii_digit()) {
            for (slug, en, ja) in [
                ("approve", "Approved", "承認"),
                ("request-changes", "Changes requested", "変更を依頼"),
                ("comment", "Commented", "コメント"),
            ] {
                if state
                    .strip_prefix("reviewed (")
                    .and_then(|s| s.strip_suffix(')'))
                    == Some(slug)
                {
                    return Some(say(
                        &format!("Review submitted on #{number}: {en}"),
                        &format!("#{number} にレビューを送信しました: {ja}"),
                    ));
                }
                if state.strip_suffix(" review unconfirmed") == Some(slug) {
                    return Some(say(
                        &format!("Review submission unconfirmed on #{number}"),
                        &format!("#{number} のレビュー送信を確認できません"),
                    ));
                }
            }
        }
    }
    for (command, en, ja) in [
        ("label list", "labels", "ラベル"),
        ("api assignees", "people", "ユーザー"),
        ("repo list", "repositories", "repository"),
        ("orgs", "organizations", "組織"),
        ("search", "GitHub items", "GitHub の項目"),
    ] {
        if text
            .strip_prefix("gh ")
            .and_then(|s| s.strip_prefix(command))
            .is_some_and(|s| s.starts_with(' '))
        {
            return Some(say(
                &format!("Couldn’t load {en} from GitHub. Try again."),
                &format!("GitHub から{ja}を読み込めませんでした。再試行してください。"),
            ));
        }
    }
    None
}

fn pr_number(text: &str) -> Option<&str> {
    let rest = text.split_once('#')?.1;
    let end = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    (end > 0).then_some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_pagination_and_preserves_repository_names() {
        for language in [Lang::En, Lang::Ja] {
            let page =
                diagnostic_for("invalid: gh json: expected issue pageInfo", language).unwrap();
            for internal in ["invalid:", "gh json", "pageInfo"] {
                assert!(!page.contains(internal), "{page}");
            }
            let repository =
                diagnostic_for("invalid repository identity: my_org/my-repo", language).unwrap();
            assert!(repository.contains("my_org/my-repo"));
            assert!(!repository.contains("identity"));
            assert!(diagnostic_for("my_org/my-repo request-changes", language).is_none());
            let cause = diagnostic_for("network: HTTP 403 my_org/my-repo", language).unwrap();
            assert!(cause.contains("HTTP 403 my_org/my-repo"));
        }
    }

    #[test]
    fn retains_uncertainty_and_check_before_retry_guidance() {
        for language in [Lang::En, Lang::Ja] {
            let text = diagnostic_for("gh pr edit #42 timed out; the reviewers, assignees and labels on #42 may or may not have changed — they were not re-read, so do not retry blindly", language).unwrap();
            assert!(text.contains("#42"));
            assert!(text.contains("GitHub"));
            assert!(text.contains("before trying again") || text.contains("再試行する前"));
        }
        assert_eq!(
            diagnostic_for("#42 reviewed (request-changes)", Lang::En).unwrap(),
            "Review submitted on #42: Changes requested"
        );
        assert_eq!(
            diagnostic_for("issue-create unconfirmed", Lang::Ja).unwrap(),
            "Issue の作成を確認できません"
        );
        assert!(diagnostic_for(
            "merge is confirmed; gh reported an error: HTTP 502",
            Lang::En
        )
        .unwrap()
        .ends_with("HTTP 502"));
    }
}
