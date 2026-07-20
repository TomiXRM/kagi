//! JA strings for `CheckoutNote` (ADR-0129 appendix §B-2, checkout half).
//!
//! §G-1 exception: [`CheckoutNote::WillDetachHead`] /
//! [`CheckoutNote::RecommendCreateBranchHereFirst`] keep the CURRENT Japanese
//! wording from the legacy producer byte-for-byte here, even though their
//! `message_en()` (kagi-domain) is NEW English text — see
//! `crates/kagi-domain/src/plan_note/checkout.rs` module docs for the
//! sanctioned exception.

use kagi_domain::plan_note::{CheckoutNote, CheckoutRecovery, CheckoutTitle, DirtyParts};

/// `「ステージ済み 2 件、変更 1 件」` — the dirty-parts fragment in JA
/// (mirrors `plan/common.rs::parts_ja`; kept local since `CheckoutNote` is
/// the only checkout-category note that needs it).
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

/// Japanese rendering of one checkout note.
pub fn note_ja(note: &CheckoutNote) -> String {
    match note {
        CheckoutNote::AlreadyCurrent { branch } => {
            format!("ブランチ '{}' はすでに現在の HEAD ブランチです。", branch)
        }
        CheckoutNote::CommitAlreadyHead => "このコミットはすでに HEAD です。".to_string(),
        CheckoutNote::CheckoutOverlap { count, files } => format!(
            "作業ツリーに、切り替え先も変更する {} 件のファイルへのローカルな変更があります: {}。\
             安全なチェックアウトは拒否されます(この競合がチェックアウトを妨げます)。\
             先に stash するかコミットしてください。",
            count, files
        ),
        CheckoutNote::DirtyCarriedOver { parts, branch } => {
            format!("{}は '{}' に引き継がれます。", parts_ja(parts), branch)
        }
        CheckoutNote::DirtyMayFail { display } => format!(
            "作業ツリーが dirty です({})。安全なチェックアウトが失敗する場合があります。\
             先に stash するかコミットしてください。",
            display
        ),
        // §G-1 exception: current Japanese wording preserved byte-for-byte.
        CheckoutNote::WillDetachHead => {
            "detached HEAD になります。新しい作業を残す場合は branch を作成してください。"
                .to_string()
        }
        CheckoutNote::RecommendCreateBranchHereFirst => {
            "Create branch here を先に使うことを推奨します。".to_string()
        }
    }
}

/// Japanese rendering of one checkout title.
pub fn title_ja(title: &CheckoutTitle) -> String {
    match title {
        CheckoutTitle::Checkout { branch } => format!("ブランチ '{}' をチェックアウト", branch),
        CheckoutTitle::CheckoutCommit { sha, summary } => format!(
            "コミット {} '{}' をチェックアウト(detached HEAD)",
            sha, summary
        ),
    }
}

/// Japanese rendering of one checkout recovery block.
pub fn recovery_ja(recovery: &CheckoutRecovery) -> String {
    match recovery {
        CheckoutRecovery::Checkout { previous } => format!(
            "問題が発生した場合は次のコマンドで '{}' に戻れます:\n  git checkout {}\n\
             HEAD の移動はすべて reflog に記録されます:\n  git reflog",
            previous, previous
        ),
        CheckoutRecovery::CheckoutCommit { previous } => format!(
            "誤って実行した場合は次のコマンドで戻れます:\n  git checkout {}\n\
             detached 状態からの新しい作業を残したい場合は、ブランチを作成してください:\n  git switch -c <name>\n\
             HEAD の移動はすべて reflog に記録されます:\n  git reflog",
            previous
        ),
    }
}
