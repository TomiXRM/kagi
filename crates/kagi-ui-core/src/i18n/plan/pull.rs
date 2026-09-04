//! JA strings for `PullNote` (ADR-0129 appendix §B-4).

use kagi_domain::plan_note::{DirtyParts, PullNote, PullRecovery, PullTitle};

/// `「stage 済み 2 件、変更 1 件」` — the dirty-parts fragment in JA
/// (mirrors `plan/common.rs::parts_ja`; pull has its own module so it stays
/// local rather than reaching into a sibling category file).
fn parts_ja(parts: &DirtyParts) -> String {
    let mut out: Vec<String> = Vec::new();
    if parts.staged > 0 {
        out.push(format!("stage 済み {} 件", parts.staged));
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
            "作業ツリーに{}があります。取得した変更が同じパスに触れない場合のみ pull を続行します。",
            parts_ja(parts)
        ),
        PullNote::NoUpstreamWithHint { branch, err } => format!(
            "branch `{}` に upstream が設定されていません: {}\n  git branch --set-upstream-to=<remote>/<branch>",
            branch, err
        ),
        PullNote::MergePrediction => {
            "merge 予測: 現在の upstream の先端は HEAD と conflict します。\
             fetch で変わる可能性があるため実行はブロックしませんが、変化がなければ安全に失敗し、リポジトリは変更されません。"
                .to_string()
        }
        PullNote::ConflictedRefOnly { count } => format!(
            "conflict ファイルが {} 件あります。この ref-only pull は作業ツリーに影響しません。",
            count
        ),
        PullNote::DirtyRefOnly => {
            "作業ツリーに変更があります。この ref-only pull は作業ツリーに影響しません。".to_string()
        }
        PullNote::NoUpstream { branch, err } => {
            format!("branch `{}` に upstream が設定されていません: {}", branch, err)
        }
        PullNote::AlreadyUpToDate { branch } => {
            format!("branch `{}` は upstream と同期済みです。", branch)
        }
        PullNote::CannotFastForward { branch } => format!(
            "branch `{}` は upstream に fast-forward できません。checkout 状態で pull すると merge されます。",
            branch
        ),
        PullNote::RemoteDiverged {
            branch,
            ahead,
            behind,
        } => format!(
            "`{}` は upstream から乖離しています(ahead {}、behind {})。pull はリモート上で merge commit を作成します。",
            branch, ahead, behind
        ),
        PullNote::RemoteDirty => {
            "リモートの作業ツリーに未 commit の変更があります。pull が失敗するか、ホスト側での conflict 解決が必要になる場合があります。"
                .to_string()
        }
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
                format!("{} を pull(最新、ローカル情報)", branch)
            } else {
                format!(
                    "`{}` を `{}` から pull({} commit 遅れ)",
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
                "最新、ローカル情報、fetch でさらに判明する場合あり".to_string()
            } else {
                format!(
                    "{} commit 遅れ、ローカル情報、fetch でさらに判明する場合あり",
                    behind
                )
            };
            format!("`{}` を `{}` から pull({})", branch, remote, behind_label)
        }
        PullTitle::PullBranchFf {
            branch,
            remote,
            behind,
        } => format!(
            "`{}` を `{}` から pull(ff-only, ref-only, {} 遅れ)",
            branch, remote, behind
        ),
    }
}

/// Japanese rendering of one pull recovery block.
pub fn recovery_ja(recovery: &PullRecovery) -> String {
    match recovery {
        PullRecovery::Pull => {
            "pull は非破壊的です。fast-forward とクリーンな merge では作業は失われません。\n\
             merge が conflict するか変更パスを上書きする場合、実行はブロックされリポジトリは変更されません。\n\
             実行後に merge commit を取り消すには:\n  git reset --hard HEAD~1\n\
             HEAD 移動は reflog に残ります:\n  git reflog"
                .to_string()
        }
        PullRecovery::PullRemote => {
            "ホスト上でホストの認証情報を使い `git pull` を実行します。conflict はホスト側で解決します。"
                .to_string()
        }
        PullRecovery::PullBranchFf { branch } => format!(
            "fast-forward を確認後、refs/heads/{} のみを更新します。作業ツリーは変更されません。\n\
             以前の先端に戻すには:\n  git branch -f {} <old-sha>",
            branch, branch
        ),
    }
}
