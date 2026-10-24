"""Read crates/kagi-git/src/resolution/drafts.rs:90-125,160-249,256-311.
Finite deterministic identity/payload/origin equality quotient, one path. Different
paths commute through BTreeMap lookups; ordering is per-path oldest-to-newest.
Undo/redo are represented by nonempty flags because only emptiness is tested.
No SHA/JSON decoder claim; this models authority/precedence after decoding.
"""
from dataclasses import dataclass, replace
from itertools import product
from checks import cli, prop
S='crates/kagi-git/src/resolution/drafts.rs'
T='crates/kagi-git/tests/conflicts/draft_revision.rs'
VARIANTS=('unknown-matches','allow-other-raw','oldest-recovery','null-not-state','coalesce-preserved','ignore-pending-recency','pre-merge-recovery')

@dataclass(frozen=True)
class File:
    identity: object='A'
    raw: bool=False
    result: object=None
    raw_result: object=None
    raw_sides: tuple=('C','I')
    draft_state: bool=False
    undo: bool=False
    redo: bool=False
    origin: object=None
    live: bool=False
    pending: bool=False
    hunk: object=None
    sides: str='same'
    binary: bool=False


def compatible(current,saved,broken=None):
    """drafts.rs:90-105; saved.raw is not a guard: payload shape is checked."""
    if current.identity!=saved.identity or (current.identity is None and broken!='unknown-matches'):
        return False
    if current.raw:
        return saved.result is None and (saved.raw_result is None or saved.raw_result in current.raw_sides or broken=='allow-other-raw')
    return saved.raw_result is None


def has_state(file,broken=None):
    """drafts.rs:108-113: explicit undone-to-null state counts without a body."""
    return ((file.draft_state and broken!='null-not-state') or file.result is not None or file.raw_result is not None or file.undo or file.redo)


def same(a,b):
    """drafts.rs:116-125 deliberately excludes origin/pending and history."""
    return (a.identity,a.draft_state,a.result,a.raw_result,a.hunk,a.sides,a.binary)==(b.identity,b.draft_state,b.result,b.raw_result,b.hunk,b.sides,b.binary)


def adopt(current,saved):
    """drafts.rs:194-203,223-232 move body/history/editor owner, not live side metadata."""
    return replace(current,result=saved.result,raw_result=saved.raw_result,hunk=saved.hunk,draft_state=True,
                   origin=saved.origin,live=saved.live,pending=saved.pending,undo=saved.undo,redo=saved.redo)


def preserve(history,file,broken=None):
    """drafts.rs:160-173: equal content deduplicates, then appends for recency."""
    if not has_state(file,broken):
        return history
    return [f for f in history if not same(f,file)]+[file]


def retain(current,previous,recoveries,broken=None):
    """drafts.rs:186-249: current live record outranks reversed recovery history."""
    matched=False
    if previous is not None:
        if compatible(current,previous,broken):
            if has_state(previous,broken):
                current=adopt(current,previous)
            matched=has_state(current,broken)
        else:
            recoveries=list(recoveries)+[previous]
    preserved=[]
    entries=list(recoveries) if broken=='oldest-recovery' else list(reversed(recoveries))
    for entry in entries:
        if not matched and has_state(entry,broken) and compatible(current,entry,broken):
            current=adopt(current,entry)
            matched=True
            continue
        if not same(current,entry):
            preserved.append(entry)
    history=[]
    for entry in reversed(preserved):
        history=preserve(history,entry,broken)
    return current,history


def autosave(current,local_recoveries,disk_recoveries,disk_file,broken=None,*,merged=None):
    """drafts.rs::autosave_json, draft_merge.rs::matches_record: compare a
    recovery against the actual post-merge record, including body overrides.
    Only disk FILE (not preserved recovery) may coalesce by matching owner.
    """
    actual=current if merged is None else merged
    history=[f for f in local_recoveries if not f.pending]
    pending=[f for f in local_recoveries if f.pending]
    for file,preserved in [(f,True) for f in disk_recoveries]+([] if disk_file is None else [(disk_file,False)]):
        if not has_state(file,broken):
            continue
        may_coalesce=not preserved or broken=='coalesce-preserved'
        compared=current if broken=='pre-merge-recovery' else actual
        superseded=same(compared,file) or (may_coalesce and has_state(current,broken) and
                    (compatible(current,file,broken) or (current.live and current.origin is not None and current.origin==file.origin)))
        if not superseded:
            history=[f for f in history if not same(f,file)]+[file]
    if broken!='ignore-pending-recency':
        for file in pending:
            history=[f for f in history if not same(f,file)]+[file]
    return history


def run(broken=None):
    unknown=prop('draft match','DraftNeverAdoptsUnknownAuthority','HOLD',S+':90-105,186-234',T+':166; conflicts/draft_unknown.rs:25,69',declared='Only known identical identity authorizes adoption.')
    shape=prop('draft match','DraftPayloadCompatibility','HOLD',S+':94-105',T+':287,451',declared='Raw payload must be one of exact live side OID/mode pairs; text cannot carry raw result.')
    null=prop('draft precedence','LatestExplicitNullOutranksOldBody','HOLD',S+':176-234',T+':207,581',declared='Latest record wins even when undone-to-None.')
    newest=prop('draft precedence','NewestCompatibleRecoveryWins','HOLD',S+':215-249',T+':479')
    owner=prop('draft serialization','UnknownOwnerCoalescenceIsFileOnly','HOLD',S+':253-290',T+':166 (authority); no exact pure owner predicate test',declared='Unknown identity may coalesce only immediately previous same-editor FILE, never preserved history.')
    pending=prop('draft serialization','OnlyPendingRecoveryOutranksDisk','HOLD',S+':260-267,303-311',T+':659; crates/kagi-git/tests/conflicts/draft_recency.rs:7')
    recency=prop('draft preservation','DedupMovesLatestToTail','HOLD',S+':160-173',T+':659')
    recovery=prop('draft serialization','TwoWriterRecoveryRetained','HOLD',
                  S+'::autosave_json; crates/kagi-git/src/resolution/draft_merge.rs::matches_record',
                  'crates/kagi-git/src/resolution/draft_storage_tests.rs::two_writer_recovery_survives_other_file_autosaves',
                  declared='An unchanged stale editor must compare recoveries to the actual serialized primary, never its pre-merge body.')
    for ci,si,raw,saved_raw,result,payload,explicit,undo,redo,raw_sides in product((None,'A','B'),(None,'A','B'),(False,True),(False,True),(None,(),('body',)),(None,'C','I','OTHER'),(False,True),(False,True),(False,True),((),('C',),('I',),('C','I'))):
        c=File(identity=ci,raw=raw,raw_sides=raw_sides)
        saved=File(identity=si,raw=saved_raw,result=result,raw_result=payload,draft_state=explicit,undo=undo,redo=redo)
        match=compatible(c,saved,broken)
        expected=ci is not None and ci==si and ((result is None and (payload is None or payload in raw_sides)) if raw else payload is None)
        shape.check(match==expected,{'current':c.__dict__,'saved':saved.__dict__,'compatible':match})
        adopted,_=retain(c,saved,[],broken)
        unknown.check(not has_state(adopted) or (ci is not None and ci==si),{'current':c.__dict__,'saved':saved.__dict__,'after':adopted.__dict__})
    records=[File(identity=identity,result=body,draft_state=explicit) for identity,body,explicit in product((None,'A','B'),(None,(),('old',),('new',)),(False,True))]
    for identity in (None,'A','B'):
        c=File(identity=identity)
        # Full history products through length three over eight equivalence records:
        # identity matched/mismatched, explicit null/empty/old/new.
        hist_records=[File(identity=i,result=r,draft_state=True) for i,r in product(('A','B'),(None,(),('old',),('new',)))]
        for length in range(4):
            for history in product(hist_records,repeat=length):
                result,_=retain(c,None,history,broken)
                eligible=[f for f in history if f.identity==identity and identity is not None]
                expected=eligible[-1] if eligible else c
                newest.check(result.result==expected.result and has_state(result)==has_state(expected),{'current':c.__dict__,'history':[f.__dict__ for f in history],'after':result.__dict__,'expected':expected.__dict__})
        for previous,old in product(records,records):
            if previous.identity==identity and identity is not None and previous.draft_state and previous.result is None:
                result,_=retain(c,previous,[old],broken)
                null.check(result.result is None and result.draft_state,{'events':['old recovery body','latest explicit null record','retain_drafts'],'current_identity':identity,'previous':previous.__dict__,'old':old.__dict__,'after':result.__dict__})
    for c_id,s_id,co,so,live,preserved in product((None,'A','B'),(None,'A','B'),(None,'X','Y'),(None,'X','Y'),(False,True),(False,True)):
        c=File(identity=c_id,result=('live',),draft_state=True,origin=co,live=live)
        disk=File(identity=s_id,result=('disk',),draft_state=True,origin=so)
        history=autosave(c,[],[disk] if preserved else [],None if preserved else disk,broken)
        can_drop=not preserved and ((c_id is not None and c_id==s_id) or (live and co is not None and co==so))
        owner.check((disk not in history)==can_drop,{'current':c.__dict__,'disk':disk.__dict__,'preserved':preserved,'serialized_history':[f.__dict__ for f in history]})
    # Exhaust all local/disk recovery orders through three records and every
    # local pending-bit pattern; stable latest occurrence defines serialization.
    from itertools import permutations
    recovery_records=[File(identity=label,result=(label,),draft_state=True) for label in ('A','B','C')]
    for local_len,disk_len in product(range(4),repeat=2):
        for local_order,disk in product(permutations(recovery_records,local_len),product(recovery_records,repeat=disk_len)):
            for flags in product((False,True),repeat=local_len):
                local=[replace(file,pending=flag) for file,flag in zip(local_order,flags)]
                history=autosave(File(identity='OTHER'),local,disk,None,broken)
                timeline=[f for f in local if not f.pending]+list(disk)+[f for f in local if f.pending]
                expected=[f for j,f in enumerate(timeline) if not any(same(f,later) for later in timeline[j+1:])]
                pending.check(history==expected,{'local':[(f.result,f.pending) for f in local],'disk':[f.result for f in disk],'expected':[(f.result,f.pending) for f in expected],'actual':[(f.result,f.pending) for f in history]})
    for a,b in product(records,records):
        history=preserve(preserve(preserve([],a,broken),b,broken),a,broken)
        if has_state(a):
            recency.check(history and same(history[-1],a) and sum(same(f,a) for f in history)==1,{'events':['preserve A','preserve B','preserve A'],'A':a.__dict__,'B':b.__dict__,'history':[f.__dict__ for f in history]})
    # Both body orderings, two known identities, disk adoption and assembled-hunk
    # overrides; three later other-file saves rotate beyond the backup window.
    for identity,early,late,kind in product(('A','B'),('first','second','third'),('first','second','third'),('disk','hunk-merge')):
        if early==late:
            continue
        first=File(identity=identity,result=(early,),draft_state=True,origin='one',live=True,
                   hunk=((0,early),) if kind=='hunk-merge' else None)
        second=replace(first,result=(late,),origin='two',live=False,
                       hunk=((1,late),) if kind=='hunk-merge' else None)
        actual=second if kind=='disk' else replace(first,result=(early,late),hunk=((0,early),(1,late)))
        history=[first]
        for saved in range(3):
            history=autosave(first,[],history,second,broken,merged=actual)
            recovery.check(sum(same(file,first) for file in history)==1,
                           {'identity':identity,'first':first.__dict__,'disk':second.__dict__,
                            'actual':actual.__dict__,'other_file_save':saved+1,
                            'history':[file.__dict__ for file in history]})
    return [unknown,shape,null,newest,owner,pending,recency,recovery]

if __name__=='__main__':
    cli(run,VARIANTS)
