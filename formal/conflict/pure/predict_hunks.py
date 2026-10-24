"""Read from crates/kagi-domain/src/resolution.rs:63-87,110-202,295-351,
542-628 and resolution/assembly.rs:12-94 at 6f9696df.
Finite deterministic bounded domain: lengths 0..3, all masks, six choices,
four orders. Arbitrary text is quotiented into unique tokens / marker tokens.
This is NOT an unbounded proof of all vector lengths or Rust execution.
"""
from copy import deepcopy
from itertools import product
from checks import cli, prop, masks

CHOICES = ('C', 'I', 'BC', 'BI', 'M', 'U')
ORDERS = ('CF', 'IF', 'CB', 'IB')
R = 'crates/kagi-domain/src/resolution.rs'
A = 'crates/kagi-domain/src/resolution/assembly.rs'
T = 'crates/kagi-domain/src/resolution/order_tests.rs'
VARIANTS = ('both-positional', 'empty-all', 'undecide-on-select', 'order-without-tie', 'resolved-choice-only', 'wrong-origin', 'emit-marker', 'reverse-regions')


def tri(bits, broken=None):
    """resolution.rs:110-131: selected==0 is checked before selected==len."""
    if broken == 'empty-all' and not bits:
        return 'All'
    return 'None' if not any(bits) else 'All' if all(bits) else 'Partial'

def marker(line):
    """crates/kagi-domain/src/checklist.rs:158-178: exact ASCII prefixes."""
    return line.startswith(('<<<<<<< ', '>>>>>>> ', '======= ')) or line == '======='


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
    return [[choice in ('C', 'BC', 'BI')] * n,
            [choice in ('I', 'BC', 'BI')] * m, order]


def state(choice, selection, side, broken=None):
    """resolution.rs:306-321: absent masks use choice, even on empty sides."""
    if selection is not None:
        return tri(selection[side], broken)
    return 'All' if choice in (('C', 'BC', 'BI') if side == 0 else ('I', 'BC', 'BI')) else 'None'


def resolved(choice, selection, broken=None):
    """resolution.rs:297-301: Some(empty selection) is resolved."""
    return choice != 'U' or (selection is not None and broken != 'resolved-choice-only')


def assemble(c, i, choice, selection=None, manual='', base=(), broken=None):
    """assembly.rs:17-46 (zip/block vs indexed/positional),48-70 (choice).
    Source tokens retain provenance; unresolved adds Manual marker wrappers.
    """
    if selection is not None:
        cb, ib, order = selection
        sides = ((c, cb, 'c'), (i, ib, 'i'))
        if order in ('IF', 'IB'):
            sides = sides[::-1]
        if order in ('CB', 'IB'):
            out = [(line, origin) for lines, bits, origin in sides for line, bit in zip(lines, bits) if bit]
        else:
            out = []
            for index in range(max(len(c), len(i))):
                for lines, bits, origin in sides:
                    if index < len(bits) and bits[index]:
                        out.append((lines[index], origin))  # Rust panics here for malformed masks.
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
    """resolution.rs:326-351,554-577: seed before bounds check; reset only on valid deselect."""
    selection = deepcopy(selection) if selection is not None else seed(n, m, choice, broken)
    changed = index is None or index < len(selection[side])
    if changed:
        if index is None:
            selection[side] = [taken] * len(selection[side])
        else:
            selection[side][index] = taken
        if (not taken or broken == 'undecide-on-select') and not any(selection[0] + selection[1]):
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
    empty = p('SideStateChoiceMaskEquivalentIncludingEmpty', 'REFUTE', R+':157-181,306-321', classification='QUESTION', implicit='Choice-only All on an empty adopted side becomes None when masks are materialized.')
    res = p('SelectionPresenceResolvesEvenEmpty', 'HOLD', R+':297-301', T+':28-62', declared='Some(selection), including zero bits, is resolved.')
    agg = p('ChildTriStateAggregation', 'HOLD', R+':124-131,536-538', R+':972')
    orderprop = p('AssembleAllFourOrdersAndProvenance', 'HOLD', A+':17-46', T+':86-113,140-178', declared='Position order or whole-side concatenation; origin retained.')
    stable = p('AssembleBlockOrderStable', 'HOLD', R+':78-85,157-173,564-577; '+A+':17-46', T+':116,128')
    clean = p('AssembleNoSyntheticMarkers', 'HOLD', A+':48-70', R+':1023', declared='Marker-free source/manual inputs receive no synthetic marker when resolved.')
    leak = p('AssembleNoMarkerLeak', 'REFUTE', A+':49-61; '+R+':297-301', R+':1023 (ordinary source only)', 'QUESTION', implicit='Resolved is a decision flag, not a marker-free verdict; manual/source marker text survives.')
    deselect = p('UndecideOnlyOnDeselect', 'HOLD', R+':334-351,554-577', R+':723,750; '+T+':54-61')
    applicable = p('OrderApplicability', 'HOLD', R+':580-627', T+':7,28,86,171')
    invalid = p('InvalidLineFailureIsNoOp', 'REFUTE', R+':326-331,564-577', classification='QUESTION', implicit='ensure_line_selection precedes set_line failure: false can still resolve an unresolved hunk.')
    malformed = p('MalformedSelectionNeverPanics', 'REFUTE', A+':29-41', classification='QUESTION', implicit='Block zip truncates; positional selection indexes beyond a shorter source. Shape must be a caller invariant.')
    converted = p('ChoiceSelectionPreservesSourceChoices', 'HOLD', R+':157-173; '+A+':17-58', R+':863; '+T+':116,128')
    regionprop = p('RegionAssemblyPreservesOrder', 'HOLD', A+':76-84', R+':789', declared='Passthrough and hunk regions preserve sequence and context provenance.')
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
            sel = [list(cb), list(ib), order]
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
                        else:
                            expected_bits[s][index] = taken
                    reset = valid and not taken and not any(expected_bits[0]+expected_bits[1])
                    expected_state = ('U',None) if reset else (choice,expected_bits)
                    deselect.check((newchoice,newsel)==expected_state and changed==valid, dict(witness,side=s,taken=taken,index=index,after=[newchoice,newsel],expected=expected_state))
        for s,taken in product((0,1),(False,True)):
            newchoice,newsel,_ = edit(n,m,choice,None,s,taken,broken=broken)
            bits = seed(n,m,choice)
            bits[s] = [taken]*len(bits[s])
            expected_reset = not taken and not any(bits[0]+bits[1])
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
    for choice in CHOICES[:-1]:
        for text in ('plain','<<<<<<< user','=======','>>>>>>> user',''):
            out=assemble([text],[text],choice,manual=text,broken=broken)
            leak.check(not any(marker(x) for x,_ in out), {'choice':choice,'text':text,'resolved':True,'actual':out})
    for n,m,cb_len,ib_len,order in product(range(4),range(4),range(5),range(5),ORDERS):
        for cb,ib in product(masks(cb_len),masks(ib_len)):
            try:
                assemble(['c']*n,['i']*m,'U',[cb,ib,order])
                ok=True
            except IndexError:
                ok=False
            malformed.check(ok, {'lengths':[n,m], 'masks':[cb,ib], 'order':order,'outcome':'Rust positional lines[index] panics' if not ok else 'ok'})
    # Each region is independent: full products of empty/context, six choices
    # and all one-line-per-side masks/orders through three ordered regions.
    variants=[('context',()),('context',('context',))]
    variants += [('hunk',(['c'],['i'],ch,None,'manual')) for ch in CHOICES]
    variants += [('hunk',(['c'],['i'],'U',[[c],[i],order])) for c,i,order in product((False,True),(False,True),ORDERS)]
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
