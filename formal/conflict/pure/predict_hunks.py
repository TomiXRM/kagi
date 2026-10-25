"""Extracted from kagi-domain resolution and assembly; #1746 C1-C4 amendments
map to LineSelection::empty_taken/side_state/new, set_hunk_line, the provenance
marker gate, and assembly's checked source lookup. Finite bounded domain:
lengths 0..3, all masks, six choices, four orders. Not an unbounded proof.
"""
from copy import deepcopy
from itertools import product
from checks import cli, prop, masks

CHOICES = ('C', 'I', 'BC', 'BI', 'M', 'U')
ORDERS = ('CF', 'IF', 'CB', 'IB')
R = 'crates/kagi-domain/src/resolution.rs'
A = 'crates/kagi-domain/src/resolution/assembly.rs'
T = 'crates/kagi-domain/src/resolution/order_tests.rs'
VARIANTS = ('both-positional', 'empty-all', 'empty-adoption-lost', 'invalid-materializes', 'malformed-index', 'plain-text-gate', 'undecide-empty-adoption', 'order-without-tie', 'resolved-choice-only', 'wrong-origin', 'emit-marker', 'reverse-regions', 'crlf-marker-missed')


def tri(bits, broken=None):
    """resolution.rs:110-131: selected==0 is checked before selected==len."""
    if broken == 'empty-all' and not bits:
        return 'All'
    return 'None' if not any(bits) else 'All' if all(bits) else 'Partial'

def marker(line, broken=None):
    """resolution.rs::resolution_text_has_conflict_marker: complete >=7 run."""
    if broken != 'crlf-marker-missed' and line.endswith('\r'):
        line = line[:-1]
    if not line or line[0] not in '<=>':
        return False
    width = len(line) - len(line.lstrip(line[0]))
    return width >= 7 and (width == len(line) or line[width] == ' ')


def marker_gate(lines, broken=None):
    """resolution::resolved_lines_have_conflict_marker: Manual-only residue."""
    return any(marker(text, broken) and (origin == 'm' or broken == 'plain-text-gate')
               for text, origin in lines)


def children(values):
    """resolution.rs:124-131,536-538: empty child list is None."""
    if not values or all(v == 'None' for v in values):
        return 'None'
    return 'All' if all(v == 'All' for v in values) else 'Partial'


def seed(n, m, choice, broken=None):
    """resolution.rs:78-85,157-173: Both seeds block order, not tie order."""
    order = 'CB' if choice == 'BC' else 'IB' if choice == 'BI' else 'CF'
    if broken == 'both-positional':
        order = {'CB': 'CF', 'IB': 'IF'}.get(order, order)
    takes = [choice in ('C', 'BC', 'BI'), choice in ('I', 'BC', 'BI')]
    return [[takes[0]] * n, [takes[1]] * m, order,
            [takes[0] and n == 0, takes[1] and m == 0]]


def state(choice, selection, side, broken=None):
    """LineSelection::side_state: empty adoption is an explicit persisted bit."""
    if selection is not None:
        if not selection[side] and selection[3][side] and broken != 'empty-adoption-lost':
            return 'All'
        return tri(selection[side], broken)
    return 'All' if choice in (('C', 'BC', 'BI') if side == 0 else ('I', 'BC', 'BI')) else 'None'


def resolved(choice, selection, broken=None):
    """resolution.rs:297-301: Some(empty selection) is resolved."""
    return choice != 'U' or (selection is not None and broken != 'resolved-choice-only')


def assemble(c, i, choice, selection=None, manual='', base=(), broken=None):
    """assembly.rs checked source/mask lookups (including stale dimensions).
    Source tokens retain provenance; unresolved adds Manual marker wrappers.
    """
    if selection is not None:
        cb, ib, order = selection[:3]
        sides = ((c, cb, 'c'), (i, ib, 'i'))
        if order in ('IF', 'IB'):
            sides = sides[::-1]
        if order in ('CB', 'IB'):
            out = [(line, origin) for lines, bits, origin in sides for line, bit in zip(lines, bits) if bit]
        else:
            out = []
            for index in range(max(len(c), len(i))):
                for lines, bits, origin in sides:
                    if index < len(bits) and bits[index] and (index < len(lines) or broken == 'malformed-index'):
                        out.append((lines[index], origin))
    elif choice == 'C':
        out = [(x, 'c') for x in c]
    elif choice == 'I':
        out = [(x, 'i') for x in i]
    elif choice in ('BC', 'BI'):
        sides = ((c, 'c'), (i, 'i')) if choice == 'BC' else ((i, 'i'), (c, 'c'))
        out = [(x, origin) for lines, origin in sides for x in lines]
    elif choice == 'M':
        lines = [] if manual == '' else manual.split('\n')
        if lines and lines[-1] == '':
            lines.pop()  # split_terminator, assembly.rs:59-61.
        out = [(x, 'm') for x in lines]
    else:
        out = [('<<<<<<< Current', 'm')] + [(x, 'm') for x in c]
        if base:
            out += [('||||||| Base', 'm')] + [(x, 'm') for x in base]
        out += [('=======', 'm')] + [(x, 'm') for x in i] + [('>>>>>>> Incoming', 'm')]
    if broken == 'wrong-origin':
        out = [(x, 'm') for x, _ in out]
    if broken == 'emit-marker' and resolved(choice, selection):
        out.append(('=======', 'm'))
    return out


def edit(n, m, choice, selection, side, taken, index=None, broken=None):
    """set_hunk_line validates before materializing; deselect respects deletion."""
    if index is not None and index >= (n if side == 0 else m) and broken != 'invalid-materializes':
        return choice, deepcopy(selection), False
    selection = deepcopy(selection) if selection is not None else seed(n, m, choice, broken)
    changed = index is None or index < len(selection[side])
    if changed:
        if index is None:
            selection[side] = [taken] * len(selection[side])
            selection[3][side] = taken and not selection[side]
        else:
            selection[side][index] = taken
        adoption = [] if broken == 'undecide-empty-adoption' else selection[3]
        if not taken and not any(selection[0] + selection[1] + adoption):
            choice, selection = 'U', None
    return choice, selection, changed


def reorder(n, m, choice, selection, order, broken=None):
    """resolution.rs:585-627: applicability before mutation; no mask creation."""
    selection = deepcopy(selection)
    changed = False
    if selection is not None:
        applicable = (any(a and b for a, b in zip(selection[0], selection[1])) if order in ('CF', 'IF')
                      else any(selection[0]) and any(selection[1]))
        if (not applicable and broken != 'order-without-tie') or selection[2] == order:
            return choice, selection, False
        selection[2] = order
        changed = True
    elif n == 0 or m == 0:
        return choice, selection, False
    if choice in ('BC', 'BI'):
        new = 'BC' if order in ('CF', 'CB') else 'BI'
        changed |= choice != new
        choice = new
    return choice, selection, changed


def assemble_regions(regions, broken=None):
    """assembly.rs:76-84: visit each region in sequence; passthrough is Context."""
    out = []
    for kind, value in (reversed(regions) if broken == 'reverse-regions' else regions):
        if kind == 'context':
            out.extend((text, 'x') for text in value)
        else:
            out.extend(assemble(*value, broken=broken))
    return out


def run(broken=None):
    properties = []
    def p(name, expected, rust, test='None found for exact property', classification='CONTRACT', declared='', implicit=''):
        item = prop('hunk/assembly', name, expected, rust, test, classification, declared, implicit)
        properties.append(item)
        return item
    side = p('SideStateConsistent', 'HOLD', R+':110-131,174-181,306-321', R+':972', declared='Masks aggregate None/All/Partial; empty masks are None.')
    empty = p('SideStateChoiceMaskEquivalentIncludingEmpty', 'HOLD', R+'::LineSelection::side_state/from_choice', R.replace('resolution.rs', 'resolution/hardening_tests.rs')+'::empty_side_state_survives_choice_materialization', declared='Adopted empty sides remain All after mask materialization.')
    res = p('SelectionPresenceResolvesEvenEmpty', 'HOLD', R+':297-301', T+':28-62', declared='Some(selection), including zero bits, is resolved.')
    agg = p('ChildTriStateAggregation', 'HOLD', R+':124-131,536-538', R+':972')
    orderprop = p('AssembleAllFourOrdersAndProvenance', 'HOLD', A+':17-46', T+':86-113,140-178', declared='Position order or whole-side concatenation; origin retained.')
    stable = p('AssembleBlockOrderStable', 'HOLD', R+':78-85,157-173,564-577; '+A+':17-46', T+':116,128')
    clean = p('AssembleNoSyntheticMarkers', 'HOLD', A+':48-70', R+':1023', declared='Marker-free source/manual inputs receive no synthetic marker when resolved.')
    leak = p('MarkerGateRespectsChosenSourceProvenance', 'HOLD', R+'::resolved_lines_have_conflict_marker', R.replace('resolution.rs', 'resolution/hardening_tests.rs')+'::marker_gate_respects_chosen_source_provenance', declared='Only manual marker text blocks; chosen source literals survive without blocking.')
    deselect = p('UndecideOnlyOnDeselect', 'HOLD', R+':334-351,554-577', R+':723,750; '+T+':54-61')
    applicable = p('OrderApplicability', 'HOLD', R+':580-627', T+':7,28,86,171')
    invalid = p('InvalidLineFailureIsNoOp', 'HOLD', R+'::HunkModel::set_hunk_line', R.replace('resolution.rs', 'resolution/hardening_tests.rs')+'::invalid_hunk_line_edit_is_a_complete_noop')
    malformed = p('MalformedSelectionNeverPanics', 'HOLD', A+'::ConflictHunk::visit_resolved_lines', R.replace('resolution.rs', 'resolution/hardening_tests.rs')+'::stale_selection_dimensions_never_panic_or_emit_missing_lines')
    converted = p('ChoiceSelectionPreservesSourceChoices', 'HOLD', R+':157-173; '+A+':17-58', R+':863; '+T+':116,128')
    regionprop = p('RegionAssemblyPreservesOrder', 'HOLD', A+':76-84', R+':789', declared='Passthrough and hunk regions preserve sequence and context provenance.')
    crlf = p('CRLFManualMarkerResidue', 'HOLD', R+'::resolution_text_has_conflict_marker', R.replace('resolution.rs', 'resolution/hardening_tests.rs')+'::crlf_manual_marker_residue_blocks_at_any_supported_width', declared='One optional CR is ignored only for marker detection; source provenance remains authoritative.')
    for width, char, ending, origin in product((6, 7, 8, 10), '<=>', ('', '\r'), ('m', 'c', 'i', 'x')):
        text = char * width + ending
        crlf.check(marker_gate([(text, origin)], broken) == (width >= 7 and origin == 'm'), {'width': width, 'marker': char, 'ending': repr(ending), 'origin': origin})
    for values_len in range(4):
        for values in product(('None', 'All', 'Partial'), repeat=values_len):
            expected = 'None' if not values or set(values) == {'None'} else 'All' if set(values) == {'All'} else 'Partial'
            agg.check(children(values) == expected, {'children': values})
    for n, m, choice in product(range(4), range(4), CHOICES):
        c, i = [f'c{x}' for x in range(n)], [f'i{x}' for x in range(m)]
        seed_bits = seed(n, m, choice, broken)
        if choice in ('C','I','BC','BI'):
            converted.check(assemble(c,i,choice,seed_bits,broken=broken) == assemble(c,i,choice,broken=broken), {'lengths':[n,m],'choice':choice,'seed':seed_bits})
        for s in (0, 1):
            empty.check(state(choice, None, s, broken) == state(choice, seed_bits, s, broken), {'lengths': [n,m], 'choice': choice, 'side': s, 'before': state(choice,None,s,broken), 'after': state(choice,seed_bits,s,broken)})
        for cb, ib, order in product(masks(n), masks(m), ORDERS):
            sel = [list(cb), list(ib), order, [False, False]]
            witness = {'lengths': [n,m], 'choice': choice, 'selection': sel}
            for s in (0, 1):
                bits = sel[s]
                expected = 'None' if sum(bits) == 0 else 'All' if sum(bits) == len(bits) else 'Partial'
                side.check(state(choice,sel,s,broken) == expected, dict(witness, side=s))
            res.check(resolved(choice,sel,broken), witness)
            out = assemble(c,i,choice,sel,broken=broken)
            tokens = [(x,'c') for x,b in zip(c,cb) if b] + [(x,'i') for x,b in zip(i,ib) if b]
            if order in ('CB','IB'):
                expected = tokens if order == 'CB' else [(x,'i') for x,b in zip(i,ib) if b] + [(x,'c') for x,b in zip(c,cb) if b]
            else:
                expected = sorted(tokens, key=lambda x: (int(x[0][1:]), (x[1] == 'i') if order=='CF' else (x[1]=='c')))
            orderprop.check(out == expected, dict(witness, actual=out, expected=expected))
            clean.check(not any(marker(x) for x,_ in out), witness)
            for request in ORDERS:
                newchoice,newsel,changed = reorder(n,m,choice,sel,request,broken)
                allowed = any(a and b for a,b in zip(cb,ib)) if request in ('CF','IF') else any(cb) and any(ib)
                applicable.check(changed == (allowed and order != request) and (not changed or newsel[:2] == sel[:2]), dict(witness, requested=request, changed=changed))
            for s,taken in product((0,1),(False,True)):
                for index in [None]+list(range(len(sel[s])+1)):
                    newchoice,newsel,changed = edit(n,m,choice,sel,s,taken,index,broken)
                    expected_bits = deepcopy(sel)
                    valid = index is None or index < len(sel[s])
                    if valid:
                        if index is None:
                            expected_bits[s] = [taken]*len(sel[s])
                            expected_bits[3][s] = taken and not expected_bits[s]
                        else:
                            expected_bits[s][index] = taken
                    reset = valid and not taken and not any(expected_bits[0]+expected_bits[1]+expected_bits[3])
                    expected_state = ('U',None) if reset else (choice,expected_bits)
                    deselect.check((newchoice,newsel)==expected_state and changed==valid, dict(witness,side=s,taken=taken,index=index,after=[newchoice,newsel],expected=expected_state))
        for s,taken in product((0,1),(False,True)):
            newchoice,newsel,_ = edit(n,m,choice,None,s,taken,broken=broken)
            bits = seed(n,m,choice)
            bits[s] = [taken]*len(bits[s])
            bits[3][s] = taken and not bits[s]
            expected_reset = not taken and not any(bits[0]+bits[1]+bits[3])
            deselect.check((newchoice,newsel) == (('U',None) if expected_reset else (choice,bits)), {'lengths':[n,m], 'choice':choice, 'side':s, 'taken':taken, 'after':[newchoice,newsel]})
            failedchoice,failedsel,changed = edit(n,m,choice,None,s,True,index=(n if s==0 else m),broken=broken)
            if choice == 'U':
                invalid.check(not changed and (failedchoice,failedsel)==(choice,None), {'events':['unresolved hunk with no line_select','set_hunk_line(side,index=len,taken=true) returns false'], 'lengths':[n,m], 'choice':choice, 'side':s, 'after':[failedchoice,failedsel], 'resolved_after':resolved(failedchoice,failedsel)})
        for request in ORDERS:
            ch,se,changed = reorder(n,m,choice,None,request,broken)
            expect = bool(n and m and choice in ('BC','BI') and choice != ('BC' if request in ('CF','CB') else 'BI'))
            applicable.check(changed == expect and se is None, {'lengths':[n,m], 'choice':choice,'requested':request})
        if choice in ('BC','BI'):
            baseline = assemble(c,i,choice)
            for s in (0,1):
                for index in range(n if s==0 else m):
                    ch,se,_ = edit(n,m,choice,None,s,False,index,broken)
                    target = (c if s==0 else i)[index]
                    expected = [line for line in baseline if line[0] != target]
                    # Last-line deselection intentionally restores unresolved markers.
                    if len(baseline)>1:
                        stable.check(assemble(c,i,ch,se,broken=broken)==expected, {'events':[f'choose {choice}',f'uncheck side {s} line {index}'], 'lengths':[n,m], 'expected':expected, 'actual':assemble(c,i,ch,se,broken=broken)})
        if choice != 'U':
            out = assemble(c,i,choice,manual='plain',broken=broken)
            clean.check(not any(marker(x) for x,_ in out), {'choice':choice,'lengths':[n,m]})
    for choice in CHOICES:
        for text in ('plain','<<<<<<< user','=======','>>>>>>> user',''):
            out=assemble([text],[text],choice,manual=text,broken=broken)
            expected = choice == 'U' or (choice == 'M' and marker(text))
            leak.check(marker_gate(out, broken) == expected, {'choice':choice,'text':text,'expected_blocked':expected,'actual':out})
    for n,m,cb_len,ib_len,order in product(range(4),range(4),range(5),range(5),ORDERS):
        for cb,ib in product(masks(cb_len),masks(ib_len)):
            try:
                assemble(['c']*n,['i']*m,'U',[cb,ib,order,[False,False]],broken=broken)
                ok=True
            except IndexError:
                ok=False
            malformed.check(ok, {'lengths':[n,m], 'masks':[cb,ib], 'order':order,'outcome':'Rust positional lines[index] panics' if not ok else 'ok'})
    # Each region is independent: full products of empty/context, six choices
    # and all one-line-per-side masks/orders through three ordered regions.
    variants=[('context',()),('context',('context',))]
    variants += [('hunk',(['c'],['i'],ch,None,'manual')) for ch in CHOICES]
    variants += [('hunk',(['c'],['i'],'U',[[c],[i],order,[False,False]])) for c,i,order in product((False,True),(False,True),ORDERS)]
    for length in range(4):
        for regions in product(variants,repeat=length):
            expected=[]
            for kind,value in regions:
                if kind=='context':
                    expected.extend((text,'x') for text in value)
                else:
                    expected.extend(assemble(*value))
            regionprop.check(assemble_regions(regions,broken)==expected, {'regions':regions,'actual':assemble_regions(regions,broken),'expected':expected})
    return properties

if __name__ == '__main__':
    cli(run, VARIANTS)
