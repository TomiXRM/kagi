use kagi::app::{self, Completion, Delivery, OperationId, Sessions};
use kagi::remote::stash::RemoteStashReport;
use kagi_domain::remote::RemoteDropOutcome;
use kagi_git::OpOutcome;

pub struct AppliedRemote {
    pub id: OperationId,
    pub report: RemoteStashReport,
}

/// Shared assertion for the finite fake G and the opt-in real-SSH M path.
pub fn assert_remote_completion(
    sessions: &mut Sessions,
    completion: Completion,
    expected: RemoteDropOutcome,
) -> AppliedRemote {
    let Completion::Stash(stash) = &completion else {
        panic!("remote stash job returned a non-stash completion")
    };
    let report = stash
        .remote_report()
        .expect("remote stash job must carry remote evidence")
        .clone();
    assert_eq!(report.evidence.outcome, expected);
    let entry = report.recording.entry();
    assert_eq!(entry.op, "stash-drop");
    assert!(matches!(
        (&entry.outcome, expected),
        (OpOutcome::Success { .. }, RemoteDropOutcome::Success)
            | (OpOutcome::Refused { .. }, RemoteDropOutcome::Refused)
            | (OpOutcome::Failed { .. }, RemoteDropOutcome::Failed)
            | (OpOutcome::Partial { .. }, RemoteDropOutcome::Partial)
            | (OpOutcome::Unknown { .. }, RemoteDropOutcome::Unknown)
    ));
    if expected == RemoteDropOutcome::Success {
        let oid = &report
            .evidence
            .frame
            .as_ref()
            .expect("success must have a terminal frame")
            .selected_oid;
        assert_eq!(oid.len(), 40, "receipt must retain a full stash OID");
        assert!(
            kagi_git::oplog::entry_to_json(entry).contains(oid),
            "success receipt must contain the full dropped stash OID"
        );
    }
    let deliveries = app::apply(sessions, completion);
    let id = deliveries
        .into_iter()
        .find_map(|delivery| match delivery {
            Delivery::RemoteCompleted { id, .. } => Some(id),
            _ => None,
        })
        .expect("remote completion delivery");
    AppliedRemote { id, report }
}
