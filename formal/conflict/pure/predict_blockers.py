"""Read from crates/kagi-git/src/conflicts.rs:803-929 and
resolution/conflict_boundary.rs:45-79; src/ui/conflict_view.rs:360-391.
Finite deterministic I/O-result quotient: read OK/error, identity unknown/equal/
different, message missing/error/empty/comments/text. File lists length 0..3,
all kinds and resolution flags; marker path subset enumerated independently
(both inside and outside the session), preserving ordered/duplicate categories.
"""
from itertools import product
from checks import cli, prop
VARIANTS=('drop-raw', 'ignore-index-error', 'reverse-blockers', 'ui-ignore-markers')
KINDS=('text','binary','symlink','gitlink','deletion','df')
SOURCE='crates/kagi-git/src/conflicts.rs:803-929; crates/kagi-git/src/resolution/conflict_boundary.rs:45-79'


def message_empty(message):
    """conflicts.rs:920-929: ALL read_to_string errors return false, not only ENOENT."""
    return message in ('empty','comments')


def predict(files, residue, unexpected, merge, message, index_ok=True, op_equal=True, context=True, identities=(), conflicts_ok=True, broken=None):
    """conflicts.rs:808-828 first return Result errors;831-908 accumulate in source order."""
    if (not index_ok and broken!='ignore-index-error') or not op_equal or not context:
        return 'ERROR', []
    if any(resolved and identity!='equal' for (_,resolved),identity in zip(files,identities)):
        return 'ERROR', []  # conflict_boundary.rs:73-77, only resolved session paths.
    out=[]
    conditions=[('unresolved-files', [str(j) for j,(_,r) in enumerate(files) if not r]),
                ('marker-residue',list(residue)),
                ('binary-unresolved',[str(j) for j,(k,r) in enumerate(files) if k in ('binary','symlink','gitlink') and not r]),
                ('deletion-undecided',[str(j) for j,(k,r) in enumerate(files) if k=='deletion' and not r])]
    for name,paths in conditions:
        if paths and not (broken=='drop-raw' and name=='binary-unresolved'):
            out.append((name,paths))
    if not conflicts_ok:
        return 'ERROR', []  # conflicts.rs:885-892 errors discard accumulated Ok list.
    if unexpected:
        out.append(('index-unmerged',list(unexpected)))
    if merge and message_empty(message):
        out.append(('empty-merge-message',[]))
    if broken=='reverse-blockers':
        out.reverse()
    return 'OK',out


def ui(files, residue, broken=None):
    """conflict_view.rs:360-391: Binary-specific (not all raw), deletion, generic, markers."""
    if any(not r for _,r in files):
        if any(k=='binary' and not r for k,r in files):
            return 'binary'
        if any(k=='deletion' and not r for k,r in files):
            return 'deletion'
        return 'unresolved'
    return 'marker' if residue and broken!='ui-ignore-markers' else None


def run(broken=None):
    complete=prop('continue blockers','BlockersOrderedAndComplete','HOLD',SOURCE,'crates/kagi-git/tests/conflicts_test.rs:911',declared='All six produced categories in source order, not enum declaration order. Raw/deletion also get generic unresolved.')
    fail=prop('continue blockers','IndexContextIdentityErrorsFailClosed','HOLD',SOURCE,declared='Unreadable index and unknown/stale authority are Result errors, never empty lists.')
    message=prop('merge message','MessageReadErrorNeverFailOpen','REFUTE','crates/kagi-git/src/conflicts.rs:914-929','None found for non-ENOENT read error','QUESTION',declared='Missing message intentionally permits synthesized summary.',implicit='Every read/UTF-8 error also means not empty: message mechanism alone fails open.')
    textmsg=prop('merge message','MessageContentGate','HOLD','crates/kagi-git/src/conflicts.rs:925-929','crates/kagi-git/tests/conflicts_test.rs:911')
    uip=prop('UI continue','UIBlockerPrecedence','HOLD','src/ui/conflict_view.rs:360-391',declared='Missing binary dominates deletion, then generic; only then cached markers.')
    # 1+12+144+1728 file vectors; each joined with all residue subsets of
    # session paths plus one outside path, two unexpected-path cases and messages.
    for n in range(4):
        for files in product(tuple(product(KINDS,(False,True))),repeat=n):
            for residue_bits in product((False,True),repeat=n+1):
                residue=[str(j) for j,b in enumerate(residue_bits) if b]
                for unexpected,merge,msg in product(((),('outside',)),(False,True),('missing','error','empty','comments','text')):
                    actual=predict(files,residue,unexpected,merge,msg,identities=('equal',)*n,broken=broken)
                    expected=[]
                    categories=[('unresolved-files',[str(j) for j,(_,r) in enumerate(files) if not r]),
                                ('marker-residue',residue),
                                ('binary-unresolved',[str(j) for j,(k,r) in enumerate(files) if k in ('binary','symlink','gitlink') and not r]),
                                ('deletion-undecided',[str(j) for j,(k,r) in enumerate(files) if k=='deletion' and not r]),
                                ('index-unmerged',list(unexpected))]
                    expected.extend((k,v) for k,v in categories if v)
                    if merge and msg in ('empty','comments'):
                        expected.append(('empty-merge-message',[]))
                    w={'files':files,'residue':residue,'unexpected':unexpected,'merge':merge,'message':msg,'actual':actual,'expected':['OK',expected]}
                    complete.check(actual==('OK',expected),w)
                expected_ui='binary' if any(k=='binary' and not r for k,r in files) else 'deletion' if any(k=='deletion' and not r for k,r in files) else 'unresolved' if any(not r for _,r in files) else 'marker' if residue else None
                uip.check(ui(files,residue,broken)==expected_ui,{'files':files,'residue':residue,'expected':expected_ui,'actual':ui(files,residue,broken)})
            for ids in product(('unknown','equal','different'),repeat=n):
                for index_ok,op_equal,context,conflicts_ok in product((False,True),repeat=4):
                    actual=predict(files,(),(),False,'text',index_ok,op_equal,context,ids,conflicts_ok,broken)
                    expected_error=not all((index_ok,op_equal,context,conflicts_ok)) or any(r and ident!='equal' for (_,r),ident in zip(files,ids))
                    fail.check((actual[0]=='ERROR')==expected_error,{'files':files,'identities':ids,'index_ok':index_ok,'operation_equal':op_equal,'context_known_equal':context,'conflict_iteration_ok':conflicts_ok,'actual':actual})
    for msg in ('missing','error','empty','comments','text'):
        actual=predict((),(),(),True,msg)
        message.check(msg!='error' or actual != ('OK',[]), {'events':['merge in progress, no unresolved files','read_to_string(MERGE_MSG) => permission/I/O/invalid UTF-8 error','merge_message_is_empty => false','continue_blockers => Ok([])'], 'message':msg,'actual':actual})
        textmsg.check(message_empty(msg)==(msg in ('empty','comments')),{'message':msg})
    return [complete,fail,message,textmsg,uip]

if __name__=='__main__':
    cli(run,VARIANTS)
