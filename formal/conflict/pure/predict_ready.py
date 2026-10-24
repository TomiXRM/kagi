"""Pure text equality/readiness from src/ui/conflict_view/flow.rs:115-122,
crates/kagi-git/src/resolution.rs:627-656 and
crates/kagi-domain/src/resolution/assembly.rs:87-92. Equality quotient is finite:
None vs empty vs one/two lines, equal/different text and each provenance.
Marker verdict is a supplied cached Boolean, not a proof of cache freshness.
"""
from itertools import product
from checks import cli, prop
VARIANTS = ('ignore-marker', 'compare-provenance', 'empty-is-missing')


def predict(result, raw, marker, model, broken=None):
    """flow.rs:117-122; resolution.rs:640-656; assembly.rs:89-92."""
    has_resolution = (result is not None if broken != 'empty-is-missing' else bool(result)) or raw
    ready = has_resolution and (not marker or broken == 'ignore-marker')
    differs = False
    if result is not None and model is not None:
        differs = (result != model if broken == 'compare-provenance'
                   else [x[0] for x in result] != [x[0] for x in model])
    return ready, differs


def run(broken=None):
    source='src/ui/conflict_view/flow.rs:115-122; crates/kagi-git/src/resolution.rs:627-656; crates/kagi-domain/src/resolution/assembly.rs:87-92'
    ready=prop('ready','ReadyImpliesNoMarkers','HOLD',source,'crates/kagi-git/tests/conflicts_test.rs:440,2254',declared='Ready is a Result/raw choice and a false cached marker verdict.',implicit='No statement about stale cached verdicts.')
    textual=prop('manual divergence','ManualDivergenceIsTextual','HOLD',source,declared='Identical text with different provenance is not divergent.')
    empty=prop('ready','EmptyResultIsAResolution','HOLD',source, 'crates/kagi-git/tests/conflicts/draft_revision.rs:207')
    absent=prop('manual divergence','MissingResultOrModelNotDivergent','HOLD',source)
    domain=[None, ()]
    for length in (1,2):
        domain += [tuple(lines) for lines in product(tuple(product(('a','b'),('c','i','m','x'))), repeat=length)]
    for result,model,raw,marker in product(domain,domain,(False,True),(False,True)):
        is_ready,diverges=predict(result,raw,marker,model,broken)
        w={'result':result,'model':model,'raw':raw,'marker':marker,'ready':is_ready,'diverges':diverges}
        ready.check(not is_ready or not marker,w)
        textual.check(diverges == (result is not None and model is not None and [x[0] for x in result] != [x[0] for x in model]),w)
        absent.check(not (result is None or model is None) or not diverges,w)
        if result == () and not marker:
            empty.check(is_ready,w)
    return [ready,textual,empty,absent]

if __name__=='__main__':
    cli(run,VARIANTS)
