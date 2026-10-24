"""crates/kagi-domain/src/sequencer_skip.rs:59-102,125-147 read directly.
Finite order/equality quotient: marker None/A/B; step None/0/1/2; all Boolean
observation flags. Step magnitudes beyond 2 cannot change <,=,> predicates.
"""
from itertools import product
from checks import cli,prop
S='crates/kagi-domain/src/sequencer_skip.rs'
VARIANTS=('ignore-broken','nonzero-always-fails','exit-alone-finishes')


def signal(before,after,step=False):
    """sequencer_skip.rs:59-78: one-sided presence/backwards step is Broken."""
    if before is None and after is None:
        return 'Absent'
    if before is None or after is None:
        return 'Broken'
    if step and after<before:
        return 'Broken'
    return 'Unchanged' if before==after else 'Advanced'


def advanced(before,after,broken=None):
    """sequencer_skip.rs:87-102: Broken dominates Advanced, then Unchanged."""
    signals=[signal(before[0],after[0]),signal(before[1],after[1],True)]
    if 'Broken' in signals and broken!='ignore-broken':
        return None
    if 'Advanced' in signals:
        return True
    if 'Unchanged' in signals:
        return False
    return None


def predict(ok,in_progress,unmerged,before,after,broken=None):
    """sequencer_skip.rs:125-147: zero exit while in flight bypasses movement."""
    if broken=='exit-alone-finishes' and ok:
        return 'Finished'
    if not (in_progress or unmerged):
        return 'Finished' if ok else 'Unclear'
    if ok:
        return 'Advanced'
    if broken=='nonzero-always-fails':
        return 'NoProgress'
    moved=advanced(before,after,broken)
    if moved is True:
        return 'Advanced' if unmerged else 'Unclear'
    return 'NoProgress' if moved is False else 'Unclear'


def run(broken=None):
    safety=prop('Skip classification','SkipClassificationNoFalseSuccess','REFUTE',S+':125-146; crates/kagi-git/src/conflicts.rs:1899-1906,1929-1937; src/ui/operations/conflict_skip.rs:129-132',S+':203 (zero-exit example WITH movement; no no-movement test)','QUESTION',declared='Exit code is evidence; mid-sequence clean exit is Advanced.',implicit='Zero exit alone accepts unchanged/absent/broken movement evidence as Advanced; UI maps Advanced to Success if the later summary read succeeds.')
    dominate=prop('Skip evidence','BrokenSignalDominatesMovement','HOLD',S+':87-102',S+':263-334',declared='A broken signal cannot be interpreted as progress.')
    nextp=prop('Skip classification','NextConflictIsSuccessOnObservedMovement','HOLD',S+':139-146',S+':176,186')
    finished=prop('Skip classification','FinishedRequiresCleanRepository','HOLD',S+':126-138',S+':218')
    unchanged=prop('Skip classification','NonzeroUnchangedIsNoProgress','HOLD',S+':139-146',S+':232')
    positions=list(product((None,'A','B'),(None,0,1,2)))
    for ok,progress,unmerged,before,after in product((False,True),(False,True),(False,True),positions,positions):
        result=predict(ok,progress,unmerged,before,after,broken)
        moved=advanced(before,after,broken)
        signals=[signal(before[0],after[0]),signal(before[1],after[1],True)]
        w={'events':['observe before','git <op> --skip returns '+('0' if ok else 'nonzero'),'observe after','classify '+result], 'ok':ok,'in_progress':progress,'unmerged':unmerged,'before':before,'after':after,'signals':signals,'classified':result}
        safety.check(not (ok and progress and moved is not True) or result not in ('Finished','Advanced'),w)
        dominate.check('Broken' not in signals or moved is None,w)
        finished.check(result!='Finished' or (ok and not progress and not unmerged),w)
        if not ok and unmerged and moved is True:
            nextp.check(result=='Advanced',w)
        if not ok and (progress or unmerged) and moved is False:
            unchanged.check(result=='NoProgress',w)
    return [safety,dominate,nextp,finished,unchanged]

if __name__=='__main__':
    cli(run,VARIANTS)
