//! Session leases over the shared composer queue, not a separate persistence layer.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, Weak};

use super::{
    composer_draft_version, draft_file_path_in_dir, drafts_dir, issue_drafts, queue_composer_draft,
    GitError, IssueCreateFields, IssueDraftKey, IssueDraftRecord,
};

mod recovery;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct DraftLocation {
    root: Option<PathBuf>,
    repo: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DraftActivity {
    Read,
    Edited,
}

#[derive(Debug)]
struct DraftScope {
    location: DraftLocation,
    slot: usize,
    activity: Mutex<BTreeMap<String, DraftActivity>>,
    lease: Weak<()>,
}

type Claims = BTreeMap<DraftLocation, Vec<Weak<()>>>;

fn claims() -> &'static Mutex<Claims> {
    static CLAIMS: LazyLock<Mutex<Claims>> = LazyLock::new(|| Mutex::new(BTreeMap::new()));
    &CLAIMS
}

fn mark_loaded(activity: &mut BTreeMap<String, DraftActivity>, branch: &str) -> bool {
    if activity.contains_key(branch) {
        return false;
    }
    activity.insert(branch.to_owned(), DraftActivity::Read);
    true
}

fn mark_edited(activity: &mut BTreeMap<String, DraftActivity>, branch: &str) {
    if let Some(state) = activity.get_mut(branch) {
        *state = DraftActivity::Edited;
    } else {
        activity.insert(branch.to_owned(), DraftActivity::Edited);
    }
}

/// A session's exclusive recovery slot for PR comments from one clone.
///
/// Construct once when the session opens, then clone for admitted posts.
/// Restoration uses a non-owning reader. Clones retain the slot until last drop. A
/// later session claims the lowest free slot, restoring its durable drafts
/// (or pending failed writes) without persisting a SessionId. Its first load
/// may recover an unclaimed sibling's draft if its own slot is empty.
/// Slot zero retains the pre-session `:pr:<base>:<number>` disk namespace.
#[derive(Clone, Debug)]
pub struct PrCommentDraftSession {
    scope: Arc<DraftScope>,
    _lease: Arc<()>,
}

/// A read may retain its captured location, but never a closed editor's slot.
#[derive(Clone, Debug)]
pub struct PrCommentDraftReader(Arc<DraftScope>);

impl PrCommentDraftReader {
    /// Return no draft after the originating session/post lease expires.
    pub fn load(&self, base_repo: &str, number: u64) -> Option<String> {
        self.0.load(base_repo, number)
    }
}

impl PrCommentDraftSession {
    /// Capture the exact drafts root and clone path, without filesystem I/O.
    pub fn new(repo: &Path) -> Self {
        let location = DraftLocation {
            root: drafts_dir(),
            repo: repo.to_path_buf(),
        };
        // Recovery takes the claim lock before the queue lock; no path takes
        // them in reverse. Dead weak leases need no drop callback.
        let mut claims = claims()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let slots = claims.entry(location.clone()).or_default();
        let slot = slots
            .iter()
            .position(|lease| lease.strong_count() == 0)
            .unwrap_or(slots.len());
        let lease = Arc::new(());
        let scope = Arc::new(DraftScope {
            location,
            slot,
            activity: Mutex::new(BTreeMap::new()),
            lease: Arc::downgrade(&lease),
        });
        let weak_lease = Arc::downgrade(&lease);
        if slot == slots.len() {
            slots.push(weak_lease);
        } else {
            slots[slot] = weak_lease;
        }
        Self {
            scope,
            _lease: lease,
        }
    }

    /// The captured clone locator, including after its UI session closes.
    pub fn repo(&self) -> &Path {
        self.scope.repo()
    }

    /// Capture a background reader without extending this session's slot lease.
    pub fn reader(&self) -> PrCommentDraftReader {
        PrCommentDraftReader(self.scope.clone())
    }

    fn key(&self, base_repo: &str, number: u64) -> IssueDraftKey {
        self.scope.key(base_repo, number)
    }

    fn touch(&self, key: &IssueDraftKey) {
        let mut activity = self
            .scope
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        mark_edited(&mut activity, &key.branch);
    }

    /// Replace this slot's pending body without filesystem I/O; empty clears.
    pub fn queue(&self, base_repo: &str, number: u64, body: &str) -> u64 {
        let key = self.key(base_repo, number);
        self.touch(&key);
        queue_composer_draft(key, "", body, &IssueCreateFields::default())
    }

    /// Read or allocate this exact key's version without filesystem I/O.
    pub fn version(&self, base_repo: &str, number: u64) -> u64 {
        composer_draft_version(self.key(base_repo, number))
    }

    /// Prefer pending content over disk, recovering an unclaimed orphan only
    /// on this scope's first load of an untouched addressed PR.
    pub fn load(&self, base_repo: &str, number: u64) -> Option<String> {
        self.scope.load(base_repo, number)
    }

    /// Retry an empty read after an older post releases its recovery slot.
    /// Edits (including deleting typed text) and confirmed clears never retry.
    /// Advance the existing token so an earlier read cannot overwrite recovery.
    pub fn retry_reader_if_untouched(
        &self,
        base_repo: &str,
        number: u64,
    ) -> Option<(u64, PrCommentDraftReader)> {
        let key = self.key(base_repo, number);
        let mut activity = self
            .scope
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if activity.get(&key.branch) != Some(&DraftActivity::Read) {
            return None;
        }
        activity.remove(&key.branch);
        let version = issue_drafts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .advance(&key);
        Some((version, self.reader()))
    }

    /// Consume only this exact scoped key's current token, queuing a clear.
    /// No filesystem I/O occurs; repeated or foreign completions return None.
    pub fn clear_if_version(&self, base_repo: &str, number: u64, version: u64) -> Option<u64> {
        let key = self.key(base_repo, number);
        // Activity -> claims (load only) -> queue is the global lock order.
        let mut activity = self
            .scope
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut queue = issue_drafts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.versions.get(&key) != Some(&version) {
            return None;
        }
        mark_edited(&mut activity, &key.branch);
        let next = queue.advance(&key);
        queue.pending.insert(key, IssueDraftRecord::default());
        Some(next)
    }

    /// Persist only this exact scoped key and token through the shared writer.
    /// A failed write remains pending, including after this lease is dropped.
    pub fn flush_if_version(
        &self,
        base_repo: &str,
        number: u64,
        version: u64,
    ) -> Result<bool, GitError> {
        let key = self.key(base_repo, number);
        let mut queue = issue_drafts()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if queue.versions.get(&key) != Some(&version) {
            return Ok(false);
        }
        // Keep the shared lock through writes and recovery dependencies, so
        // an old flush cannot resurrect a cleared draft or lose an orphan.
        queue.persist_pending(&key)
    }
}

impl DraftScope {
    fn repo(&self) -> &Path {
        &self.location.repo
    }

    fn key(&self, base_repo: &str, number: u64) -> IssueDraftKey {
        self.key_at_slot(&base_repo.to_ascii_lowercase(), number, self.slot)
    }

    fn key_at_slot(&self, base_repo: &str, number: u64, slot: usize) -> IssueDraftKey {
        let branch = if slot == 0 {
            format!(":pr:{base_repo}:{number}")
        } else {
            format!(":pr-slot:{slot}:{base_repo}:{number}")
        };
        IssueDraftKey {
            path: self
                .location
                .root
                .as_deref()
                .map(|root| draft_file_path_in_dir(self.repo(), &branch, root)),
            repo: self.repo().to_path_buf(),
            branch,
            number: Some(number),
        }
    }

    fn load(&self, base_repo: &str, number: u64) -> Option<String> {
        if self.lease.strong_count() == 0 {
            return None;
        }
        let key = self.key(base_repo, number);
        // Keep concurrent readers serialized, without retaining the editor lease.
        let mut activity = self
            .activity
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let first = mark_loaded(&mut activity, &key.branch);
        recovery::load(self, key, base_repo, number, first)
    }
}
