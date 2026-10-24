"""src/ui/operations/conflict.rs:140-170,859-869; src/app/flow.rs:204-214,
261-299,312-324. Finite deterministic equality quotient. Attachment is six
components (tab, incarnation, path, worktree, visit, presence); equality is
structural. No interleaving between approve and begin_write is assumed here.
"""
from itertools import product
from checks import cli,prop
VARIANTS=('intent-or','ignore-visit','ignore-pane','ignore-policy','ignore-reconcile','ignore-lease')
S='src/ui/operations/conflict.rs'
F='src/app/flow.rs'


def intent(token,revision,owner,pane,broken=None):
    """conflict.rs:159-167,867-869: compare Some(frozen values), conjunctively."""
    frozen=(0,0,'repo','worktree',0)
    if broken=='ignore-visit' and owner is not None:
        owner=owner[:4]+(0,)
    terms=(token=='A',revision=='A',owner==frozen)
    matches=any(terms) if broken=='intent-or' else all(terms)
    return matches and (pane or broken=='ignore-pane')


def approve(ready,token,policy,attached,attachment,worktree,observation,broken=None):
    """flow.rs:266-293 and identity_matches:204-214, no lease check until begin_write."""
    if not ready or not token or (not policy and broken!='ignore-policy') or not attached:
        return False
    return attachment and worktree and observation


def begin(approved_state,current_revision,reconcile,lease,reserve_ok,broken=None):
    """flow.rs:313-324: state/revision, reconcile scope, ANY existing lease, reserve."""
    if not approved_state or not current_revision:
        return False
    if reconcile and broken!='ignore-reconcile':
        return False
    if lease and broken!='ignore-lease':
        return False
    return reserve_ok


def run(broken=None):
    admission=prop('UI intent','IntentAdmissionConjunctive','HOLD',S+':159-170,859-869',S+':890',declared='Current token, full revision, whole Attachment and pane mutation admission must all match.')
    approval=prop('plan admission','PlanApprovalConjunctive','HOLD',F+':204-214,261-299','tests/app_writer_admission_test.rs (integration family; not executed)',declared='Ready/current token-policy, attached identity, worktree and accepted conflict observation all required.')
    write=prop('write admission','BeginWriteConjunctive','HOLD',F+':312-324','tests/app_writer_admission_test.rs (integration family; not executed)',declared='Current approved state/revision, no same-scope reconcile, no lease and successful reservation.')
    owners=[None]+[(a,b,p,w,v) for a,b,p,w,v in product((0,1),(0,1),('repo','other'),('worktree','other',None),(0,1))]
    for token,revision,owner,pane in product((None,'A','B'),(None,'A','B'),owners,(False,True)):
        expected=token=='A' and revision=='A' and owner==(0,0,'repo','worktree',0) and pane
        actual=intent(token,revision,owner,pane,broken)
        admission.check(actual==expected,{'current_token':token,'current_revision':revision,'current_owner':owner,'frozen_owner':[0,0,'repo','worktree',0],'pane_admitted':pane,'admitted':actual})
    for inputs in product((False,True),repeat=7):
        actual=approve(*inputs,broken)
        approval.check(actual==all(inputs),{'ready_token_policy_attached_attachment_worktree_observation':inputs,'approved':actual})
    for inputs in product((False,True),repeat=5):
        actual=begin(*inputs,broken)
        expected=inputs[0] and inputs[1] and not inputs[2] and not inputs[3] and inputs[4]
        write.check(actual==expected,{'approved_revision_reconcile_lease_reserve':inputs,'admitted':actual})
    return [admission,approval,write]

if __name__=='__main__':
    cli(run,VARIANTS)
