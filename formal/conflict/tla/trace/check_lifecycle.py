"""Bounded six-process TLC runner; each scenario keeps its original cfg.

Independent TLC processes retain exact per-check counterexamples and exit codes.
The concurrency cap avoids 48 JVM startups in series without unbounded fan-out.
No product builds or repository commands are run.
"""
from concurrent.futures import ThreadPoolExecutor
import json
import os
import re
import subprocess
import sys
import tempfile
import time

NEW_CHECKS = {'skip-refresh-broken', 'skip-confirm-cli-race',
              'continue-metadata-read', 'continue-metadata-read-broken'}


def run(checks, root):
    mutation_check = '--broken' in sys.argv[1:]
    selected = set(sys.argv[1:]) - {'--broken'}
    if selected - {c[0] for c in checks}:
        print('Unknown checks: ' + repr(selected), file=sys.stderr)
        return 2
    out = root / ('broken-results' if mutation_check else 'results')
    out.mkdir(exist_ok=True)
    start = time.monotonic()
    chosen = [c for c in checks if (not selected or c[0] in selected)
              and (not mutation_check or c[4] != 'none')]

    def check(c):
        name, scenario, inv, expected, broken = c
        if mutation_check:
            expected = 'HOLD'
        cfg_root = root / 'trace' if name in NEW_CHECKS else root
        check_out = out
        check_out.mkdir(exist_ok=True)
        cmd = ['java', '-XX:+UseSerialGC',
               '-XX:ActiveProcessorCount=1', '-Xms32m', '-Xmx256m',
               '-cp', os.environ['TLA2TOOLS_JAR'],
               'tlc2.TLC', '-workers', '1', '-seed', '1', '-fp', '0',
               '-metadir', str(check_out / (name + '-states')), '-config', str(cfg_root / (name + '.cfg')),
               str(root / 'ConflictLifecycle.tla')]
        t0 = time.monotonic()
        try:
            # TLC 1.7.4 extracts standard modules into java.io.tmpdir using
            # shared filenames; isolate each JVM so peers cannot truncate them.
            with tempfile.TemporaryDirectory(prefix='kagi-tlc-') as tmp:
                p = subprocess.run([cmd[0], f'-Djava.io.tmpdir={tmp}', *cmd[1:]],
                                   cwd=root, text=True, stdout=subprocess.PIPE,
                                   stderr=subprocess.STDOUT, timeout=55)
                text, code = p.stdout, p.returncode
        except (subprocess.TimeoutExpired, OSError) as exc:
            text, code = str(exc), None
        elapsed = round(time.monotonic()-t0, 3)
        (check_out / (name + '.log')).write_text(text)
        violated = (f'Invariant {inv} is violated' in text or
                    f'Temporal property {inv} was violated' in text or
                    'Temporal properties were violated' in text)
        held = code == 0 and 'Model checking completed. No error has been found.' in text
        actual = ('REFUTE' if violated and code == (13 if inv == 'ContinueEventuallyRecordedOrReported' else 12)
                  else 'HOLD' if held else 'ERROR')
        counts = re.findall(r'([\d,]+) states generated, ([\d,]+) distinct states found', text)
        generated, distinct = (tuple(int(n.replace(',', '')) for n in counts[-1]) if counts else (None, None))
        events = []
        for block in re.split(r'(?m)^State \d+:.*\n', text)[1:]:
            m = re.search(r'event\s*\|->\s*(<<.*?>>|"[^"]*")', block, re.S)
            if m:
                events.append(re.sub(r'\s+', ' ', m[1]))
        cycle = re.search(r'(Back to state \d+|Stuttering)', text)
        if cycle:
            events.append(cycle[1])
        if actual == 'REFUTE' and not events:
            actual = 'ERROR'
        result = dict(name=name, scenario=scenario, invariant=inv, expectation=expected,
                      result=actual, broken=broken, seconds=elapsed, states=distinct, generated=generated,
                      tlc_exit=code, trace=events,
                      log=str((check_out / (name + '.log')).relative_to(root.parent)))
        (check_out / (name + '.json')).write_text(json.dumps(result, indent=2) + '\n')
        return result

    with ThreadPoolExecutor(max_workers=6) as pool:
        results = list(pool.map(check, chosen))
    for r in results:
        print(f"{'PASS' if r['result']==r['expectation'] else 'FAIL'} {r['name']}: {r['result']} expected={r['expectation']} {r['seconds']}s", flush=True)
    summary = dict(seconds=round(time.monotonic()-start, 3), checks=results, workers=6,
                   distinct_states_sum=sum(r['states'] or 0 for r in results),
                   ok=bool(results) and all(r['result'] == r['expectation'] for r in results))
    (out / ('summary-selected.json' if selected else 'summary.json')).write_text(json.dumps(summary, indent=2) + '\n')
    print(f"{len(results)} checks; {summary['distinct_states_sum']} distinct states; {summary['seconds']}s")
    return 2 if any(r['result'] == 'ERROR' for r in results) else 0 if summary['ok'] else 1
