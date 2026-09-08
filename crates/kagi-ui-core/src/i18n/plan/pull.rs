//! JA strings for `PullNote` (ADR-0129 appendix §B-4).

use kagi_domain::plan_note::{
    restore_conflict_paths, DirtyParts, PullNote, PullRecovery, PullTitle,
};

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

/// `<要約>` + 1 行 1 パス + `<助言>`。両方の restore note が共有する形。
fn path_note_ja(summary: &str, advice: &str, paths: &[String]) -> String {
    let (shown, extra) = restore_conflict_paths(paths);
    let mut out = String::from(summary);
    for path in shown {
        out.push_str("\n  - ");
        out.push_str(path);
    }
    if extra > 0 {
        out.push_str(&format!("\n  - 他 {extra} 件"));
    }
    out.push_str(advice);
    out
}

/// Japanese rendering of one pull note.
pub fn note_ja(note: &PullNote) -> String {
    match note {
        PullNote::DirtyPullGuard { parts } => format!(
            "作業ツリーに{}があります。取得した変更が同じパスに触れない場合のみ pull を続行します。",
            parts_ja(parts)
        ),
        PullNote::AutoStash { parts, untracked } => {
            let mut changes = Vec::new();
            let tracked = parts_ja(parts);
            if !tracked.is_empty() {
                changes.push(tracked);
            }
            if *untracked > 0 {
                changes.push(format!("未追跡 {} 件", untracked));
            }
            format!(
                "作業ツリーに{}があります。Kagi は変更を stash してから pull し、その後に復元します。復元が conflict した場合、stash は保持されます。",
                changes.join("、")
            )
        }
        PullNote::NoUpstreamWithHint { branch, err } => format!(
            "branch `{}` に upstream が設定されていません: {}\n  git branch --set-upstream-to=<remote>/<branch>",
            branch, err
        ),
        PullNote::MergePrediction => {
            "merge 予測: 現在の upstream の先端は HEAD と conflict します。\
             fetch で変わる可能性があるため実行はブロックしませんが、変化がなければ安全に失敗し、リポジトリは変更されません。"
                .to_string()
        }
        PullNote::RestoreConflict { paths } => path_note_ja(
            "pull 後の stash 復元は conflict します。あなたの編集と incoming の変更を merge した結果、次のパスは merge できません:",
            "\n先に commit か stash するか、pull 後に conflict を解決してください。stash はどちらでも保持されます。",
            paths,
        ),
        PullNote::RestoreConflictPossible { paths } => path_note_ja(
            "pull 後の stash 復元は conflict する可能性があります。次のパスは両方で変更されており、事前に merge を判定できませんでした(binary、mode 変更、片側での追加・削除):",
            "\n復元は試行されます。conflict した場合、stash は保持されます。",
            paths,
        ),
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
            "`{}` は upstream から乖離しています(ahead {}、behind {})。pull はremote上で merge commit を作成します。",
            branch, ahead, behind
        ),
        PullNote::RemoteDirty => {
            "remoteの作業ツリーに未 commit の変更があります。pull が失敗するか、ホスト側での conflict 解決が必要になる場合があります。"
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
             実行後に履歴を書き換えず merge commit を取り消すには:\n  git revert -m 1 HEAD\n\
             HEAD 移動は reflog に残ります:\n  git reflog"
                .to_string()
        }
        PullRecovery::PullAutoStash => {
            "pull の前に stage 済み・未 stage・未追跡の変更を stash し、pull 後にその一時 stash を pop します。\n\
             pull が失敗した場合も、結果を表示する前に stash の復元を試みます。\n\
             復元が conflict した場合は、保存した作業を失わないよう stash を保持します。"
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #625: JA must name the paths too — a localized count is still a count.
    #[test]
    fn restore_conflict_ja_names_the_paths() {
        let text = note_ja(&PullNote::RestoreConflict {
            paths: vec!["shared.txt".into(), "src/lib.rs".into()],
        });
        assert!(text.contains("\n  - shared.txt"), "{text}");
        assert!(text.contains("\n  - src/lib.rs"), "{text}");
        assert!(text.contains("conflict"), "{text}");
    }

    #[test]
    fn restore_conflict_ja_counts_what_it_truncates() {
        let paths: Vec<String> = (0..15).map(|i| format!("f{i}.txt")).collect();
        let text = note_ja(&PullNote::RestoreConflict { paths });
        assert!(text.contains("\n  - f0.txt"), "{text}");
        assert!(text.contains("他 3 件"), "{text}");
    }

    /// #625 (review): JA must keep the same distinction — 断定 と 可能性.
    #[test]
    fn restore_conflict_ja_separates_certain_from_possible() {
        let paths = vec!["shared.txt".to_string()];
        let certain = note_ja(&PullNote::RestoreConflict {
            paths: paths.clone(),
        });
        let possible = note_ja(&PullNote::RestoreConflictPossible { paths });

        assert!(certain.contains("conflict します"), "{certain}");
        assert!(!certain.contains("可能性"), "{certain}");
        assert!(possible.contains("可能性があります"), "{possible}");
        assert!(certain.contains("\n  - shared.txt"), "{certain}");
        assert!(possible.contains("\n  - shared.txt"), "{possible}");
    }
    #[test]
    fn pull_recovery_reverts_without_rewriting_history() {
        let text = recovery_ja(&PullRecovery::Pull);
        assert!(text.contains("git revert -m 1 HEAD"));
        assert!(!text.contains("reset --hard"));
    }
}
