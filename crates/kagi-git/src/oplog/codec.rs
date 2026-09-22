//! Durable oplog wire schema. Domain types stay free of serde dependencies.
//!
//! Keep legacy scalar/default handling at this boundary; append, identity
//! reconstruction, and retention continue to own their existing policies.

use super::{recovery, Actor, FailureCode, OpLogEntry, OpOutcome, StateSummary};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    failure_code: Option<&'static str>,
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
    #[serde(default, deserialize_with = "failure_code")]
    failure_code: Option<FailureCode>,
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
        failure_code: entry.failure_code.map(FailureCode::as_str),
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
        failure_code: record.failure_code,
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
