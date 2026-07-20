//! JA strings for `PullNote` (ADR-0129 appendix §B-4).

use kagi_domain::plan_note::{DirtyParts, PullNote, PullRecovery, PullTitle};

/// `「ステージ済み 2 件、変更 1 件」` — the dirty-parts fragment in JA
/// (mirrors `plan/common.rs::parts_ja`; pull has its own module so it stays
/// local rather than reaching into a sibling category file).
fn parts_ja(parts: &DirtyParts) -> String {
    let mut out: Vec<String> = Vec::new();
    if parts.staged > 0 {
        out.push(format!("ステージ済み {} 件", parts.staged));
    }
    if parts.modified > 0 {
        out.push(format!("変更 {} 件", parts.modified));
    }
    out.join("、")
}

/// Japanese rendering of one pull note.
pub fn note_ja(note: &PullNote) -> String {
    match note {
        PullNote::DirtyPullGuard { parts } => format!(
            "作業ツリーに{}があります。取得した変更が同じパスに触れない場合のみ pull は続行されます。",
            parts_ja(parts)
        ),
        PullNote::NoUpstreamWithHint { branch, err } => format!(
            "ブランチ '{}' に upstream が設定されていません: {}。\
             `git branch --set-upstream-to=<remote>/<branch>` で設定してください。",
            branch, err
        ),
        PullNote::MergePrediction => {
            "プラン時点でのマージ予測: 現在の upstream の先端は HEAD とコンフリクトします。\
             実行はブロックされません(fetch で状況が変わる可能性があるため)が、\
             upstream に変化がなければ実行は安全に失敗し、リポジトリは変更されません。"
                .to_string()
        }
        PullNote::ConflictedRefOnly { count } => format!(
            "リポジトリに {} 件のコンフリクトファイルがあります。この ref-only pull は作業ツリーに影響しません。",
            count
        ),
        PullNote::DirtyRefOnly => {
            "作業ツリーに変更があります。この ref-only pull は作業ツリーに影響しません。".to_string()
        }
        PullNote::NoUpstream { branch, err } => {
            format!("ブランチ '{}' に upstream が設定されていません: {}。", branch, err)
        }
        PullNote::AlreadyUpToDate { branch } => {
            format!("ブランチ '{}' は upstream と同期済みです。", branch)
        }
        PullNote::CannotFastForward { branch } => format!(
            "ブランチ '{}' は upstream に fast-forward できません。チェックアウトした状態で pull すると merge されます。",
            branch
        ),
        PullNote::RemoteDiverged {
            branch,
            ahead,
            behind,
        } => format!(
            "{} は upstream から乖離しています(ahead {} 件、behind {} 件)。\
             pull はリモート上で merge コミットを作成します。",
            branch, ahead, behind
        ),
        PullNote::RemoteDirty => "リモートの作業ツリーに未コミットの変更があります。pull が失敗するか、\
             ホスト側での解決が必要なコンフリクトが発生する可能性があります。"
            .to_string(),
    }
}

/// Japanese rendering of one pull title.
pub fn title_ja(title: &PullTitle) -> String {
    match title {
        PullTitle::PullRemote {
            branch,
            upstream,
            behind,
        } => {
            if *behind == 0 {
                format!("{} を pull — 最新です(ローカル情報)", branch)
            } else {
                format!(
                    "{} を {} から pull — {} コミット遅れ",
                    branch, upstream, behind
                )
            }
        }
        PullTitle::Pull {
            branch,
            remote,
            behind,
        } => {
            let behind_label = if *behind == 0 {
                "最新です(ローカル情報、fetch でさらに判明する場合あり)".to_string()
            } else {
                format!(
                    "{} コミット遅れ(ローカル情報、fetch でさらに判明する場合あり)",
                    behind
                )
            };
            format!("'{}' を '{}' から pull({})", branch, remote, behind_label)
        }
        PullTitle::PullBranchFf {
            branch,
            remote,
            behind,
        } => format!(
            "'{}' を '{}' から pull(ff-only, ref-only, {} 遅れ)",
            branch, remote, behind
        ),
    }
}

/// Japanese rendering of one pull recovery block.
pub fn recovery_ja(recovery: &PullRecovery) -> String {
    match recovery {
        PullRecovery::Pull => {
            "pull は非破壊的です: fast-forward とクリーンな merge では作業は失われません。\n\
             作業ツリーの変更パスは、取得した更新と照合してからチェックアウトされます。\n\
             merge がコンフリクトするか変更パスを上書きする場合、実行はブロックされ、リポジトリは変更されません。\n\
             実行後に merge コミットを取り消すには:\n  git reset --hard HEAD~1\n\
             reflog にはすべての HEAD の移動が記録されます:\n  git reflog"
                .to_string()
        }
        PullRecovery::PullRemote => {
            "ホスト上でホスト自身の認証情報を使って `git pull` を実行します。\
             コンフリクトはホスト側での解決に委ねられます。"
                .to_string()
        }
        PullRecovery::PullBranchFf { branch } => format!(
            "fast-forward であることを確認した後、refs/heads/{} のみを更新します。作業ツリーは変更されません。\
             必要であれば、以前の先端に戻すには git branch -f {} <old-sha> を使用してください。",
            branch, branch
        ),
    }
}
