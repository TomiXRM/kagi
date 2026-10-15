//! JA strings for `TagNote`/`TagTitle`/`TagRecovery` (create-tag-here).

use kagi_domain::plan_note::tag::TagNameError;
use kagi_domain::plan_note::{TagNote, TagRecovery, TagTitle};

use crate::i18n::Msg;

pub(crate) const ADVICE_TAG_NAME_ERROR_EMPTY: &str = "tag 名を入力してください。";
pub(crate) const ADVICE_TAG_NAME_ERROR_LEADING_DASH: &str =
    "tag 名が '-' で始まっており、フラグと誤認される可能性があります。\ntag `{}`";
pub(crate) const ADVICE_TAG_PUSH_REMOTE_SIDE_EFFECT: &str =
    "tag を {} に公開します。他の人から見えるようになります。\ntag `{}`";
pub(crate) const ADVICE_TAG_PUSH_REJECTED_IF_MOVED: &str =
    "remote 側で別の commit を指して既に存在する場合、push は拒否されます。\ntag `{}`";

/// Japanese rendering of one tag-name validation error.
fn name_error_ja(err: &TagNameError) -> String {
    match err {
        TagNameError::Empty => super::advice_text(Msg::AdviceTagNameErrorEmpty, &[]),
        TagNameError::InvalidRef(name) => {
            format!("有効な tag 名ではありません。\ntag `{}`", name)
        }
        TagNameError::LeadingDash(name) => {
            super::advice_text(Msg::AdviceTagNameErrorLeadingDash, &[name])
        }
        TagNameError::Exists(name) => format!("tag はすでに存在します。\ntag `{}`", name),
        TagNameError::NamespaceCollision { name, existing } => {
            format!(
                "既存の ref `{}` と名前空間が競合しています。\ntag `{}`",
                existing, name
            )
        }
    }
}

/// Japanese rendering of one tag note.
pub fn note_ja(note: &TagNote) -> String {
    match note {
        TagNote::NameError(e) => name_error_ja(e),
        TagNote::PushIdentityChanged => {
            "承認した tag または push 先が変更されました。計画を更新してから push してください。"
                .to_string()
        }
        TagNote::CommitMissing { sha } => {
            format!("commit がこのリポジトリにありません。\ncommit `{}`", sha)
        }
        TagNote::NotFound { name } => format!("ローカルに tag がありません。\ntag `{}`", name),
        TagNote::NoRemote => "remote が未設定で、tag の push 先がありません。".to_string(),
        TagNote::PushRemoteSideEffect { remote, name } => {
            super::advice_text(Msg::AdviceTagPushRemoteSideEffect, &[remote, name])
        }
        TagNote::PushRejectedIfMoved { name } => {
            super::advice_text(Msg::AdviceTagPushRejectedIfMoved, &[name])
        }
    }
}

/// Legacy Japanese bytes for persisted notes, independent of display wording.
pub(crate) fn note_ja_contract(note: &TagNote) -> String {
    match note {
        TagNote::PushRemoteSideEffect { remote, name } => format!(
            "tag を {} に公開します。他の tag 操作と違い、このマシンの外に出て他の人から見えます。\ntag `{}`",
            remote, name
        ),
        TagNote::PushRejectedIfMoved { name } => format!(
            "remote 側で別の commit を指して既に存在する場合、push は拒否されます(移動しません)。kagi は tag を force push しません。\ntag `{}`",
            name
        ),
        _ => note_ja(note),
    }
}

/// Japanese rendering of one tag title.
pub fn title_ja(title: &TagTitle) -> String {
    match title {
        TagTitle::CreateTag { name, at } => format!("tag `{}` を {} に作成", name, at),
        TagTitle::PushTag { name, remote } => format!("tag `{}` を `{}` に push", name, remote),
    }
}

/// Japanese rendering of one tag recovery block.
pub fn recovery_ja(recovery: &TagRecovery) -> String {
    match recovery {
        TagRecovery::CreateTag { name } => format!(
            "tag `{}` は副作用なく削除できます:\n  git tag -d {}",
            name, name
        ),
        TagRecovery::PushTag { name, remote } => format!(
            "remote から tag を削除できます:\n  git push {} --delete {}\nただし他の人が fetch する前に限ります。pull 済みの tag は取り消せません。",
            remote, name
        ),
    }
}

/// Complete display-only publication scope, shared by the card and Copy all.
/// Transport authority keeps the original URLs; only this projection redacts.
pub fn push_scope_rows(identity: &kagi_domain::plan::TagPushIdentity) -> Vec<String> {
    use kagi_domain::{plan::TagKind, text_safety::sanitize_control_bytes};
    let kind = match identity.kind {
        TagKind::Lightweight => "lightweight",
        TagKind::Annotated => "annotated",
    };
    let mut rows = vec![
        format!(
            "{}: {}",
            Msg::PlanTagName.t(),
            sanitize_control_bytes(&identity.name)
        ),
        format!("{}: {kind}", Msg::PlanTagKind.t()),
        format!("{}: {}", Msg::PlanTagObject.t(), identity.object_oid),
        format!("{}: {}", Msg::PlanTagTarget.t(), identity.peeled_oid),
        format!(
            "{}: {}",
            Msg::PlanTagRemote.t(),
            sanitize_control_bytes(&identity.remote)
        ),
    ];
    rows.extend(identity.push_urls.iter().map(|url| {
        format!(
            "{}: {}",
            Msg::PlanTagPushDestination.t(),
            display_push_url(url)
        )
    }));
    rows
}

/// Git accepts URL, scp-style SSH and filesystem destinations. Redact complete
/// userinfo (a username alone can be a token) and opaque query/fragment values.
/// Do this before control-byte escaping so secrets never enter display text.
fn display_push_url(url: &str) -> String {
    let (without_fragment, fragment) = url
        .split_once('#')
        .map_or((url, false), |(base, _)| (base, true));
    let (base, query) = without_fragment
        .split_once('?')
        .map_or((without_fragment, false), |(base, _)| (base, true));
    let mut out = String::with_capacity(base.len());
    if let Some((scheme, rest)) = base.split_once("://") {
        out.push_str(scheme);
        out.push_str("://");
        let end = rest.find('/').unwrap_or(rest.len());
        let (authority, path) = rest.split_at(end);
        if let Some((_, host)) = authority.rsplit_once('@') {
            out.push_str("***@");
            out.push_str(host);
        } else {
            out.push_str(authority);
        }
        out.push_str(path);
    } else if let Some((host, path)) = base.split_once(':').filter(|(host, _)| !host.contains('/'))
    {
        if let Some((_, host)) = host.rsplit_once('@') {
            out.push_str("***@");
            out.push_str(host);
        } else {
            out.push_str(host);
        }
        out.push(':');
        out.push_str(path);
    } else {
        out.push_str(base);
    }
    if query {
        out.push_str("?***");
    }
    if fragment {
        out.push_str("#***");
    }
    kagi_domain::text_safety::sanitize_control_bytes(&out)
}

#[cfg(test)]
mod scope_tests {
    use super::display_push_url;

    #[test]
    fn contract_preserves_legacy_bytes_and_raw_tag_payloads() {
        use super::{note_ja_contract, TagNote};

        assert_eq!(
            note_ja_contract(&TagNote::PushRemoteSideEffect {
                remote: "ssh://user:secret@example/{}\n\u{1b}".into(),
                name: "tag{}\n\u{1b}".into(),
            }),
            "tag を ssh://user:secret@example/{}\n\u{1b} に公開します。他の tag 操作と違い、このマシンの外に出て他の人から見えます。\ntag `tag{}\n\u{1b}`"
        );
        assert_eq!(
            note_ja_contract(&TagNote::PushRejectedIfMoved {
                name: "tag{}\n\u{1b}".into(),
            }),
            "remote 側で別の commit を指して既に存在する場合、push は拒否されます(移動しません)。kagi は tag を force push しません。\ntag `tag{}\n\u{1b}`"
        );
    }

    #[test]
    fn push_tag_scope_redacts_secrets_without_changing_the_destination() {
        for (input, expected) in [
            (
                "https://user:password@example/repo.git?token=secret#private",
                "https://***@example/repo.git?***#***",
            ),
            (
                "https://token@example/repo.git",
                "https://***@example/repo.git",
            ),
            (
                "ssh://git:secret@[::1]:2222/repo",
                "ssh://***@[::1]:2222/repo",
            ),
            ("git@example:org/repo.git", "***@example:org/repo.git"),
            ("file:///tmp/repo.git", "file:///tmp/repo.git"),
            ("/tmp/user@repo.git", "/tmp/user@repo.git"),
            ("https://example/日本語.git", "https://example/日本語.git"),
            (
                "https://secret@example/repo\u{1b}",
                "https://***@example/repo\\x1B",
            ),
        ] {
            assert_eq!(display_push_url(input), expected);
        }
    }
}
