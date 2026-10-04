//! Durable oplog wire schema. Domain types stay free of serde dependencies.
//!
//! Keep legacy scalar/default handling at this boundary; append, identity
//! reconstruction, and retention continue to own their existing policies.

use super::{
    plan_recovery, recovery, Actor, FailureCode, OpLogEntry, OpOutcome, RecordedIdentity, RefScope,
    RepoIdentity, StateSummary,
};
use kagi_domain::github::IssueCreateFields;
use kagi_domain::ref_moves::RefMove;
use serde::{de::Error, Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

#[derive(Serialize, Deserialize)]
#[serde(remote = "StateSummary")]
struct StateRecord {
    #[serde(default, deserialize_with = "default_scalar")]
    head: String,
    #[serde(default, deserialize_with = "default_scalar")]
    dirty: String,
}

#[derive(Deserialize)]
#[serde(remote = "StateSummary")]
struct RequiredStateRecord {
    #[serde(deserialize_with = "required_scalar")]
    head: String,
    #[serde(deserialize_with = "required_scalar")]
    dirty: String,
}

fn read_state<'de, D: Deserializer<'de>>(d: D) -> Result<StateSummary, D::Error> {
    let object = Map::<String, Value>::deserialize(d)?;
    StateRecord::deserialize(Value::Object(object)).map_err(D::Error::custom)
}

fn read_required_state<'de, D: Deserializer<'de>>(d: D) -> Result<StateSummary, D::Error> {
    let object = Map::<String, Value>::deserialize(d)?;
    RequiredStateRecord::deserialize(Value::Object(object)).map_err(D::Error::custom)
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "OpOutcome", tag = "kind")]
enum OutcomeRecord {
    Success {
        #[serde(
            serialize_with = "StateRecord::serialize",
            deserialize_with = "read_state"
        )]
        after: StateSummary,
    },
    Partial {
        #[serde(
            serialize_with = "StateRecord::serialize",
            deserialize_with = "read_state"
        )]
        after: StateSummary,
        #[serde(default, deserialize_with = "default_scalar")]
        error: String,
    },
    Unknown {
        #[serde(
            serialize_with = "StateRecord::serialize",
            deserialize_with = "read_required_state"
        )]
        after: StateSummary,
        #[serde(deserialize_with = "required_scalar")]
        evidence: String,
    },
    Failed {
        #[serde(default, deserialize_with = "default_scalar")]
        error: String,
    },
    Refused {
        #[serde(default, deserialize_with = "read_blockers")]
        blockers: Vec<String>,
    },
}

// Borrow the receipt during append: summaries, errors and recovery payloads do
// not need to be cloned or assembled into intermediate JSON strings.
#[derive(Serialize)]
struct EntryRef<'a> {
    id: u64,
    parent: Option<u64>,
    timestamp: i64,
    op: &'a str,
    repo: &'a str,
    actor: &'static str,
    worktree: Option<&'a str>,
    #[serde(serialize_with = "StateRecord::serialize")]
    before: &'a StateSummary,
    #[serde(serialize_with = "OutcomeRecord::serialize")]
    outcome: &'a OpOutcome,
    backup_refs: &'a [String],
    #[serde(serialize_with = "recovery::serialize")]
    recovery: &'a [recovery::RecoveryHandle],
    #[serde(
        skip_serializing_if = "Option::is_none",
        serialize_with = "plan_recovery::serialize"
    )]
    recovery_plan: Option<&'a kagi_domain::plan_note::PlanRecovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<&'static str>,
    // Written whenever recorded, an empty list included: "nothing moved" is
    // a record, not the absence of one.
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_moves: Option<Vec<RefMoveRef<'a>>>,
    /// A missing or unknown scope must not make a legacy branch-only
    /// `ref_moves: []` sufficient to restore local tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    ref_scope: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    repo_identity: Option<RepoIdentityRef<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    issue_fields: Option<IssueFieldsRef<'a>>,
}

/// #894: `{"common_dir": "...", "dev": n, "ino": n, "born_s": n, "born_ns": n}`;
/// dev/ino only on unix, born_* only where the filesystem reports a creation
/// time (#900 review).
#[derive(Serialize)]
struct RepoIdentityRef<'a> {
    common_dir: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ino: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    born_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    born_ns: Option<u32>,
}

/// Strict: a key this version does not know may change what the identity
/// means, so it makes the field `Invalid`, not silently ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RepoIdentityRecord {
    common_dir: String,
    #[serde(default, deserialize_with = "present")]
    dev: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    ino: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    born_s: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    born_ns: Option<u32>,
}

/// A key that is absent reads as `None` (`#[serde(default)]`); a key that is
/// present must hold a value — an explicit `null` is a broken record, not a
/// missing one (#900 review).
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Serialize)]
struct IssueFieldsRef<'a> {
    labels: &'a [String],
    assignees: &'a [String],
}

#[derive(Deserialize)]
struct IssueFieldsRecord {
    #[serde(default)]
    labels: Vec<String>,
    #[serde(default)]
    assignees: Vec<String>,
}

#[derive(Serialize)]
struct RefMoveRef<'a> {
    refname: &'a str,
    old: Option<&'a str>,
    new: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    old_symbolic: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    new_symbolic: Option<&'a str>,
}

#[derive(Deserialize)]
struct RefMoveRecord {
    refname: String,
    old: Option<String>,
    new: Option<String>,
    #[serde(default)]
    old_symbolic: Option<String>,
    #[serde(default)]
    new_symbolic: Option<String>,
}

#[derive(Deserialize)]
struct EntryRecord {
    #[serde(default, deserialize_with = "optional_number")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "optional_number")]
    parent: Option<u64>,
    #[serde(deserialize_with = "timestamp")]
    timestamp: i64,
    #[serde(deserialize_with = "required_scalar")]
    op: String,
    #[serde(deserialize_with = "required_scalar")]
    repo: String,
    #[serde(default, deserialize_with = "actor")]
    actor: Actor,
    #[serde(default, deserialize_with = "worktree")]
    worktree: Option<String>,
    #[serde(deserialize_with = "read_state")]
    before: StateSummary,
    #[serde(deserialize_with = "OutcomeRecord::deserialize")]
    outcome: OpOutcome,
    // Unlike additive display/recovery fields, malformed roots must reject the
    // row so destructive retention cannot mistake them for an empty root set.
    #[serde(default)]
    backup_refs: Vec<String>,
    #[serde(default, deserialize_with = "recovery::deserialize")]
    recovery: Vec<recovery::RecoveryHandle>,
    #[serde(default, deserialize_with = "plan_recovery::deserialize")]
    recovery_plan: Option<kagi_domain::plan_note::PlanRecovery>,
    #[serde(default, deserialize_with = "failure_code")]
    failure_code: Option<FailureCode>,
    #[serde(default, deserialize_with = "ref_moves")]
    ref_moves: Option<Vec<RefMove>>,
    #[serde(default, deserialize_with = "ref_scope")]
    ref_scope: RefScope,
    // Additive: a line without the field is `Absent` (attributed as before by
    // opening its worktree). A field that is there but unreadable is
    // `Invalid` — never `Absent`, whose path fallback could take another
    // repository's entry for this one (#900 review).
    #[serde(default, deserialize_with = "repo_identity")]
    repo_identity: RecordedIdentity,
    #[serde(default, deserialize_with = "issue_fields")]
    issue_fields: Option<IssueCreateFields>,
}

pub(super) fn to_json(entry: &OpLogEntry) -> String {
    let record = EntryRef {
        id: entry.id,
        parent: entry.parent,
        timestamp: entry.timestamp,
        op: &entry.op,
        repo: &entry.repo,
        actor: entry.actor.as_str(),
        worktree: entry.worktree.as_deref(),
        before: &entry.before,
        outcome: &entry.outcome,
        backup_refs: &entry.backup_refs,
        recovery: &entry.recovery,
        recovery_plan: entry.recovery_plan.as_ref(),
        failure_code: entry.failure_code.map(FailureCode::as_str),
        ref_moves: entry.ref_moves.as_ref().map(|moves| {
            moves
                .iter()
                .map(|m| RefMoveRef {
                    refname: &m.refname,
                    old: m.old.as_deref(),
                    new: m.new.as_deref(),
                    old_symbolic: m.old_symbolic.as_deref(),
                    new_symbolic: m.new_symbolic.as_deref(),
                })
                .collect()
        }),
        ref_scope: (entry.ref_scope == RefScope::HeadsAndTags && entry.ref_moves.is_some())
            .then_some("heads-and-tags"),
        repo_identity: match &entry.repo_identity {
            RecordedIdentity::Known(id) => Some(RepoIdentityRef {
                common_dir: &id.common_dir,
                dev: id.file_id.map(|(dev, _)| dev),
                ino: id.file_id.map(|(_, ino)| ino),
                born_s: id.created.map(|(s, _)| s),
                born_ns: id.created.map(|(_, ns)| ns),
            }),
            RecordedIdentity::Absent | RecordedIdentity::Invalid => None,
        },
        issue_fields: entry.issue_fields.as_ref().map(|fields| IssueFieldsRef {
            labels: &fields.labels,
            assignees: &fields.assignees,
        }),
    };
    // This fixed schema contains only strings, integers, sequences and objects;
    // no fallible map keys, floating-point values, or custom fallible payloads.
    serde_json::to_string(&record).expect("oplog wire records are JSON-compatible")
}

pub(super) fn from_value(value: Value) -> Option<OpLogEntry> {
    // serde structs can also deserialize positional arrays. The durable schema
    // has always required objects, including its nested outcome/state records.
    if !value.is_object() || !value.get("outcome")?.is_object() {
        return None;
    }
    let record: EntryRecord = serde_json::from_value(value).ok()?;
    Some(OpLogEntry {
        id: record.id.unwrap_or(0),
        parent: record.parent,
        timestamp: record.timestamp,
        op: record.op,
        repo: record.repo,
        actor: record.actor,
        worktree: record.worktree,
        before: record.before,
        outcome: record.outcome,
        backup_refs: record.backup_refs,
        recovery: record.recovery,
        recovery_plan: record.recovery_plan,
        failure_code: record.failure_code,
        ref_moves: record.ref_moves,
        ref_scope: record.ref_scope,
        repo_identity: record.repo_identity,
        issue_fields: record.issue_fields,
    })
}

// Earlier readers accepted primitive JSON scalars as text. Preserve that
// compatibility without reviving substring searches into nested objects.
fn scalar(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Null => Some("null".to_string()),
        Value::Array(_) | Value::Object(_) => None,
    }
}

fn required_scalar<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    scalar(Value::deserialize(d)?).ok_or_else(|| D::Error::custom("expected a scalar"))
}

fn default_scalar<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(scalar(Value::deserialize(d)?).unwrap_or_default())
}

fn optional_number<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.parse().ok(),
        _ => None,
    })
}

fn timestamp<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    match Value::deserialize(d)? {
        Value::Number(number) => number
            .as_i64()
            .ok_or_else(|| D::Error::custom("invalid timestamp")),
        Value::String(text) => text.parse().map_err(D::Error::custom),
        _ => Err(D::Error::custom("invalid timestamp")),
    }
}

fn actor<'de, D: Deserializer<'de>>(d: D) -> Result<Actor, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(value.as_str().map(Actor::from_wire).unwrap_or_default())
}

fn worktree<'de, D: Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Ok(scalar(Value::deserialize(d)?).filter(|text| text != "null"))
}

fn failure_code<'de, D: Deserializer<'de>>(d: D) -> Result<Option<FailureCode>, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(Some(
        value
            .as_str()
            .map(FailureCode::from_str_lossy)
            .unwrap_or(FailureCode::Other),
    ))
}

fn read_blockers<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::Array(items) => items
            .into_iter()
            .filter_map(|item| match item {
                Value::String(text) => Some(text),
                _ => None,
            })
            .collect(),
        _ => Vec::new(),
    })
}

// Additive: a missing, null or malformed list reads as "not recorded", which
// the panel shows as an estimate. A bad record must never drop the whole row,
// and never turns into a confident "nothing moved".
fn ref_moves<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Vec<RefMove>>, D::Error> {
    let records: Option<Vec<RefMoveRecord>> = serde_json::from_value(Value::deserialize(d)?).ok();
    Ok(records.map(|records| {
        records
            .into_iter()
            .map(|r| RefMove {
                refname: r.refname,
                old: r.old,
                new: r.new,
                old_symbolic: r.old_symbolic,
                new_symbolic: r.new_symbolic,
            })
            .collect()
    }))
}

fn ref_scope<'de, D: Deserializer<'de>>(d: D) -> Result<RefScope, D::Error> {
    let value = Value::deserialize(d)?;
    Ok(if value.as_str() == Some("heads-and-tags") {
        RefScope::HeadsAndTags
    } else {
        RefScope::LegacyOrUnknown
    })
}

/// Only called when the key is present (a missing one is `Default`, i.e.
/// `Absent`). Anything that is not a whole, valid identity is `Invalid` —
/// never a `Known` value that compares as another repository, which would
/// drop a corrupt entry of this one from a restore's range silently
/// (#900 review). The rules are all in [`valid_identity`].
fn repo_identity<'de, D: Deserializer<'de>>(d: D) -> Result<RecordedIdentity, D::Error> {
    let identity = serde_json::from_value::<RepoIdentityRecord>(Value::deserialize(d)?)
        .ok()
        .and_then(valid_identity);
    Ok(identity.map_or(RecordedIdentity::Invalid, RecordedIdentity::Known))
}

/// The decode-time rules for a recorded identity, in one place. Unknown keys
/// and wrong types are already refused by `RepoIdentityRecord`
/// (`deny_unknown_fields`, typed fields); on top of that:
/// - `common_dir` is a non-empty absolute path on this platform;
/// - `dev` and `ino` come together, or not at all;
/// - `born_s` and `born_ns` come together, or not at all, and `born_ns` is
///   below one second.
fn valid_identity(r: RepoIdentityRecord) -> Option<RepoIdentity> {
    fn pair<A, B>(a: Option<A>, b: Option<B>) -> Option<Option<(A, B)>> {
        match (a, b) {
            (Some(a), Some(b)) => Some(Some((a, b))),
            (None, None) => Some(None),
            _ => None,
        }
    }
    if r.common_dir.is_empty() || !std::path::Path::new(&r.common_dir).is_absolute() {
        return None;
    }
    let file_id = pair(r.dev, r.ino)?;
    let created = pair(r.born_s, r.born_ns)?;
    if created.is_some_and(|(_, ns)| ns >= 1_000_000_000) {
        return None;
    }
    Some(RepoIdentity {
        common_dir: r.common_dir,
        file_id,
        created,
    })
}

// Additive like `ref_moves`: missing, null or malformed reads as "not
// recorded" and never drops the row (#904 review).
fn issue_fields<'de, D: Deserializer<'de>>(d: D) -> Result<Option<IssueCreateFields>, D::Error> {
    let record: Option<IssueFieldsRecord> = serde_json::from_value(Value::deserialize(d)?).ok();
    Ok(record
        .map(|r| IssueCreateFields {
            labels: r.labels,
            assignees: r.assignees,
        })
        .filter(|fields| !fields.is_empty()))
}
