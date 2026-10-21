//! First-load recovery of closed sessions through the existing pending queue.

use super::super::{load_draft_at, parse_issue_message, Draft, IssueDraftQueue};
use super::{claims, issue_drafts, IssueDraftKey, IssueDraftRecord, PrCommentDraftSession};

pub(super) fn load(
    scope: &PrCommentDraftSession,
    key: IssueDraftKey,
    base_repo: &str,
    number: u64,
    first: bool,
) -> Option<String> {
    if !first {
        let queue = issue_drafts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        return read_record(&queue, &key)
            .filter(|record| !record.is_empty())
            .map(|record| record.body);
    }
    // Claims precede the queue lock everywhere: a new session cannot claim a
    // candidate between the liveness check and the atomic pending transfer.
    let claims = claims()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut queue = issue_drafts()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(record) = read_record(&queue, &key).filter(|record| !record.is_empty()) {
        return Some(record.body);
    }
    // An outstanding source clear must not import its own destination back.
    // Keeping it authoritative until persistence prevents dependency cycles.
    if queue.recovered_to.contains_key(&key) {
        return None;
    }
    let slots = claims.get(&scope.0.location)?;
    let available = |slot: usize| {
        slot != scope.0.slot
            && slots
                .get(slot)
                .is_none_or(|lease| lease.strong_count() == 0)
    };
    let base_repo = base_repo.to_ascii_lowercase();
    let (source, record) = candidate(scope, &queue, &base_repo, number, available)?;
    let body = record.body.clone();
    // Preserve the destination's pre-load version, just as Issue migration
    // does. The restore caller captured it before dispatching this read.
    queue.pending.insert(key.clone(), record);
    queue.advance(&source);
    queue
        .pending
        .insert(source.clone(), IssueDraftRecord::default());
    queue.recovered_to.insert(source, key);
    Some(body)
}

fn read_record(queue: &IssueDraftQueue, key: &IssueDraftKey) -> Option<IssueDraftRecord> {
    if let Some(record) = queue.pending.get(key) {
        return Some(record.clone());
    }
    let draft = key.path.as_deref().and_then(load_draft_at)?;
    record_from_draft(key, draft)
}

fn record_from_draft(key: &IssueDraftKey, draft: Draft) -> Option<IssueDraftRecord> {
    if draft.mode != "issue-composer"
        || draft.repo != key.repo.to_string_lossy()
        || draft.branch != key.branch
    {
        return None;
    }
    parse_issue_message(&draft.message)
}

fn slot_for_branch(branch: &str, suffix: &str) -> Option<usize> {
    if branch.strip_prefix(":pr:") == Some(suffix) {
        return Some(0);
    }
    let (slot, rest) = branch.strip_prefix(":pr-slot:")?.split_once(':')?;
    let slot = slot.parse::<usize>().ok()?;
    (slot > 0 && rest == suffix).then_some(slot)
}

fn candidate(
    scope: &PrCommentDraftSession,
    queue: &IssueDraftQueue,
    base_repo: &str,
    number: u64,
    available: impl Fn(usize) -> bool,
) -> Option<(IssueDraftKey, IssueDraftRecord)> {
    let mut best: Option<(usize, IssueDraftKey, IssueDraftRecord)> = None;
    let suffix = format!("{base_repo}:{number}");
    for (key, record) in &queue.pending {
        let Some(slot) = slot_for_branch(&key.branch, &suffix) else {
            continue;
        };
        if record.is_empty()
            || !available(slot)
            || best
                .as_ref()
                .is_some_and(|(current, _, _)| *current <= slot)
            || *key != scope.key_at_slot(base_repo, number, slot)
        {
            continue;
        }
        best = Some((slot, key.clone(), record.clone()));
    }
    if let Some(entries) = scope
        .0
        .location
        .root
        .as_deref()
        .and_then(|root| std::fs::read_dir(root).ok())
    {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(draft) = load_draft_at(&path) else {
                continue;
            };
            let Some(slot) = slot_for_branch(&draft.branch, &suffix) else {
                continue;
            };
            if !available(slot)
                || best
                    .as_ref()
                    .is_some_and(|(current, _, _)| *current <= slot)
            {
                continue;
            }
            let key = scope.key_at_slot(base_repo, number, slot);
            if key.path.as_deref() != Some(path.as_path()) || queue.pending.contains_key(&key) {
                continue;
            }
            if let Some(record) = record_from_draft(&key, draft).filter(|r| !r.is_empty()) {
                best = Some((slot, key, record));
            }
        }
    }
    best.map(|(_, key, record)| (key, record))
}
