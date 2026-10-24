"""One entry point: baseline expectations green; EVERY mutation must exit 1.
Writes only pure/results.json; the reconciled documentation ledger is immutable.
"""
import importlib
import json
from pathlib import Path
import subprocess
import sys

HERE=Path(__file__).resolve().parent
MODULES=('predict_hunks','predict_ready','predict_blockers','predict_drafts','predict_skip','predict_intent')


def execute(module,broken=None):
    command=[sys.executable,str(HERE/(module+'.py')),'--self-check']
    if broken:
        command+=['--broken',broken]
    result=subprocess.run(command,text=True,capture_output=True,cwd=HERE,timeout=30)
    try:
        output=json.loads(result.stdout)
    except (ValueError,TypeError):
        raise RuntimeError(f'{module} {broken}: invalid output, exit {result.returncode}: {result.stderr} {result.stdout}')
    return result.returncode,output




def main():
    baselines=[]
    mutations=[]
    failures=[]
    for name in MODULES:
        module=importlib.import_module(name)
        code,data=execute(name)
        baselines.append({'model':name,'exit':code,**data})
        if code!=0 or not data['properties'] or not all(
            p['matched_expectation'] and p['domain_size'] > 0 and
            (p['result'] != 'REFUTE' or p['counterexample'] is not None)
            for p in data['properties']
        ):
            failures.append(f'{name}: invalid baseline or exit {code}')
        for variant in module.VARIANTS:
            code,data=execute(name,variant)
            mismatches=[p for p in data['properties'] if not p['matched_expectation']]
            mutations.append({'model':name,'variant':variant,'exit':code,'detected':code==1 and bool(mismatches),'mismatches':mismatches})
            if code!=1 or not mismatches:
                failures.append(f'{name} --broken {variant}: expected exit 1 with property mismatch, got {code}')
    rows=[p for b in baselines for p in b['properties']]
    total=sum(p['domain_size'] for p in rows)
    result={'baseline_ok':not failures,'baseline_property_evaluations':total,'property_count':len(rows),'mutation_count':len(mutations),'baselines':baselines,'mutations':mutations,'failures':failures}
    (HERE/'results.json').write_text(json.dumps(result,indent=2,sort_keys=True)+'\n')
    print(json.dumps({'ok':not failures,'properties':len(rows),'baseline_property_evaluations':total,'mutations':len(mutations),'failures':failures},sort_keys=True))
    return 1 if failures else 0

if __name__=='__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as exc:
        print(f"TOOL ERROR: {exc}", file=sys.stderr)
        raise SystemExit(2)
