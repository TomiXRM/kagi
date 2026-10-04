//! Operation-queue strip and toast words (#355 stage 3, ADR-0204 決定 2).
//! Labels only: no explanatory sentences (user policy). Operation names
//! (`checkout`) stay English in both languages, as in the busy labels.
use super::{lang, Lang};

/// One fixed word of the queue strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueText {
    /// Prefix of the toast for an accepted intent.
    Queued,
    /// Label above the frozen subject and body in a queued commit confirmation.
    FrozenMessage,
    /// `queued: N` — intents whose write has not started (決定 2).
    Count,
    Running,
    Planning,
    Confirming,
    Starting,
    WaitWrite,
    WaitPlan,
    WaitConfirm,
    WaitReconcile,
    WaitPull,
    /// The cancel list heading.
    Cancelled,
    CancelAll,
    Clear,
    Remove,
    Full,
    ReasonRemoved,
    ReasonRejected,
    ReasonPlanError,
    ReasonChain,
    ReasonOwnerGone,
    ReasonIdentity,
    ReasonStale,
}

pub fn queue_text(key: QueueText) -> &'static str {
    use QueueText::*;
    match (lang(), key) {
        (Lang::En, Queued) => "Queued",
        (Lang::Ja, Queued) => "キューに追加",
        (Lang::En, FrozenMessage) => "Queued message",
        (Lang::Ja, FrozenMessage) => "キューに追加したメッセージ",
        (Lang::En, Count) => "queued",
        (Lang::Ja, Count) => "待ち",
        (Lang::En, Running) => "running",
        (Lang::Ja, Running) => "実行中",
        (Lang::En, Planning) => "planning",
        (Lang::Ja, Planning) => "計画中",
        (Lang::En, Confirming) => "confirming",
        (Lang::Ja, Confirming) => "確認中",
        (Lang::En, Starting) => "starting",
        (Lang::Ja, Starting) => "開始中",
        (Lang::En, WaitWrite) => "waiting: write",
        (Lang::Ja, WaitWrite) => "待機: 書き込み",
        (Lang::En, WaitPlan) => "waiting: plan",
        (Lang::Ja, WaitPlan) => "待機: plan",
        (Lang::En, WaitConfirm) => "waiting: confirm",
        (Lang::Ja, WaitConfirm) => "待機: 確認",
        (Lang::En, WaitReconcile) => "waiting: reconcile",
        (Lang::Ja, WaitReconcile) => "待機: reconcile",
        (Lang::En, WaitPull) => "waiting: pull",
        (Lang::Ja, WaitPull) => "待機: pull",
        (Lang::En, Cancelled) => "cancelled",
        (Lang::Ja, Cancelled) => "取り消し",
        (Lang::En, CancelAll) => "Cancel all",
        (Lang::Ja, CancelAll) => "すべて取り消す",
        (Lang::En, Clear) => "Clear",
        (Lang::Ja, Clear) => "消去",
        (Lang::En, Remove) => "Remove",
        (Lang::Ja, Remove) => "外す",
        (Lang::En, Full) => "Queue full",
        (Lang::Ja, Full) => "キューが満杯",
        (Lang::En, ReasonRemoved) => "removed",
        (Lang::Ja, ReasonRemoved) => "外した",
        (Lang::En, ReasonRejected) => "declined",
        (Lang::Ja, ReasonRejected) => "確認で中止",
        (Lang::En, ReasonPlanError) => "plan failed",
        (Lang::Ja, ReasonPlanError) => "plan に失敗",
        (Lang::En, ReasonChain) => "previous step failed",
        (Lang::Ja, ReasonChain) => "前の操作が不成功",
        (Lang::En, ReasonOwnerGone) => "tab closed",
        (Lang::Ja, ReasonOwnerGone) => "tab を閉じた",
        (Lang::En, ReasonIdentity) => "worktree changed",
        (Lang::Ja, ReasonIdentity) => "worktree が変わった",
        (Lang::En, ReasonStale) => "plan outdated",
        (Lang::Ja, ReasonStale) => "plan が古い",
    }
}
