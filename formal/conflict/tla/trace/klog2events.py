#!/usr/bin/env python3
"""Extract auditable partial observations; TLC checks the runtime quotient.

Usage: python klog2events.py --extract [--trials PATH] ; python klog2events.py
The default checks saved events.json, so archived evidence needs no live apps.
No timestamps/offsets join actions.json to stderr: lanes retain their own order,
not a fabricated cross-lane chronology. Null metadata is unknown, not absent.
Unlogged steps are gaps, never synthesized approvals or successful autosaves.
"""
from pathlib import Path
import argparse
import hashlib
import json
import os
import sys
import re
import subprocess
import time

ROOT = Path(__file__).resolve().parent
OUT = ROOT / 'results'

def digest(data):
    return hashlib.sha256(data).hexdigest()

def event(kind, op='', before='', after='', **evidence):
    return dict(kind=kind, op=op, before=before, after=after, **evidence)

def map_line(line):
    if not line.startswith('[kagi] '):
        return None
    text = line[7:]
    m = re.fullmatch(r'conflict-mode: (\S+) (\d+) file\(s\)', text)
    if m:
        return event('Detect', m[1], unresolved=int(m[2]))
    exact = {
        'conflict-mode: merge resolved — ready to commit': ('Detect', 'merge'),
        'conflict-mode: cleared': ('Clear', ''),
        'conflict-mode: skip armed (second confirm required)': ('ArmSkip', ''),
        'conflict-mode: abort armed (second confirm required)': ('ArmAbort', ''),
        'refreshed (external change)': ('Refresh', ''),
    }
    if text in exact:
        return event(*exact[text])
    if 'operation metadata could not be read' in text and text.startswith('refused:'):
        return event('MetadataRefused')
    if 'opening continue confirmation (sequencer)' in text:
        return event('Review', text.split('-continue:')[0])
    if 'routing to commit message panel (merge)' in text:
        return event('MergePanel', 'merge')
    m = re.fullmatch(r'executed: (.+)-(continue|skip|abort)', text)
    if m:
        return event('Execute' + m[2].title(), m[1])
    if text.startswith('executed: merge commit '):
        return event('MergeCommit', 'merge')
    if text.startswith('footer: conflict-save:') and '→ staged (stage 0)' in text:
        return event('Save', text.split(':')[2])
    for prefix, kind in [
        ('conflict-mode: choice failed', 'ChoiceFailed'),
        ('conflict-mode: choice ', 'Choice'),
        ('conflict-mode: resolution draft ', 'DraftFailure'),
        ('footer:', 'Footer'), ('refused:', 'Failure'),
        ('oplog: write failed', 'Failure'), ('reload:', 'Refresh'),
        ('reload_external:', 'Refresh'), ('watcher: working-tree changed', 'Refresh'),
        ('conflict-mode: launch external tool:', 'Inspection'),
        ('conflict-editor: external tool requested', 'Inspection'),
        ('conflict-view: image decoded', 'Inspection'),
        ('bottom-panel: open (Failed auto-open)', 'Ancillary'),
        ('commit template:', 'Ancillary'), ('draft:', 'Ancillary'),
        ('smart-commit:', 'Ancillary')]:
        if text.startswith(prefix):
            return event(kind)
    if re.match(r'(rebase|merge|cherry-pick|revert|stash)-(continue|skip|abort).*failed:', text):
        return event('Failure')
    if text.startswith(('merge commit failed:', 'merge commit: unexpected outcome')):
        return event('Failure')
    if text.startswith(('conflict-mode:', 'conflict-editor:', 'executed:', 'footer:')):
        return event('UNMAPPED')  # fail closed; never silently drop lifecycle lines
    return None  # startup/network/graph chatter is outside map section 7

GAPS = [
    'No common timestamp/log-offset: klog and actions lanes cannot be totally ordered.',
    'No per-hunk/manual-edit/autosave-success klog: screenshots and persisted buffer.json substitute, not Git status hashes.',
    'Freeze/second-confirm/lease/preflight/WT-write/index-write/append/release are unlogged: footer plus before/after Git witnesses constrain the aggregate, not their interleaving.',
    'Witness status digest is not worktree bytes; metadata null means absent OR unreadable. Use ls-files --stage/blob/permission observations in drill reports.',
    'Detection logs omit revision: unchanged refresh retains arm; changed revision disarms. Runtime quotient carries both candidates.',
]

def extract(base):
    trials = sorted(p for p in base.iterdir() if p.is_dir() and
                    (p.name.startswith(('seq-', 'Leave', 'preexisting-')) or p.name == 'mc1'))
    result = []
    for p in trials:
        events, sources, unmapped = [], [], []
        logs = sorted(p.glob('stderr.prev.*.log')) + [p / 'stderr.log']
        for log in logs:
            raw = log.read_bytes()
            sources.append(dict(path=str(log), sha256=digest(raw), bytes=len(raw)))
            events.append(event('Start', source=str(log), line=0))
            for n, line in enumerate(raw.decode(errors='replace').splitlines(), 1):
                e = map_line(line)
                if e:
                    e.update(source=str(log), line=n, raw=line)
                    events.append(e)
                    if e['kind'] == 'UNMAPPED':
                        unmapped.append(e)
        actions_path = p / 'actions.json'
        raw = actions_path.read_bytes()
        sources.append(dict(path=str(actions_path), sha256=digest(raw), bytes=len(raw)))
        actions = json.loads(raw)
        events.append(event('Start', source=str(actions_path), line=0))
        previous = None
        for a in actions:
            before = json.dumps(a['before'], sort_keys=True, separators=(',', ':'))
            after = json.dumps(a['after'], sort_keys=True, separators=(',', ':'))
            evidence = dict(source=str(actions_path), sequence=a['sequence'], args=a['args'])
            if previous is None:
                events.append(event('WitnessInit', after=before, **evidence))
            elif previous != before:
                events.append(event('ExternalObserved', before=previous, after=before, **evidence))
            events.append(event('WitnessUnchanged' if before == after else 'WitnessChanged',
                                before=before, after=after, **evidence))
            previous = after
        gaps = list(GAPS)
        if len(logs) > 1:
            gaps.append('Relaunch replaced actions.json; stderr.log may contain an old-process tail. Each raw log is checked separately; duplicate-looking tails are not invented new executions.')
        result.append(dict(trial=p.name, events=events, sources=sources,
                           unmapped=unmapped, gaps=gaps, log_files=len(logs)))
    if len(result) < 20:
        raise ValueError('Fewer than 20 drill trials found')
    (ROOT / 'events.json').write_text(json.dumps(result, ensure_ascii=False, indent=2) + '\n')
    return result

def tla_event(e):
    # Hash opaque witness records for compact TLC states, preserving full JSON above.
    fields = {k: (digest(e[k].encode()) if k in ('before', 'after') and e[k] else e[k])
              for k in ('kind', 'op', 'before', 'after')}
    return '[' + ', '.join(k + ' |-> ' + json.dumps(v) for k, v in fields.items()) + ']'

def sequence(events):
    return '<<\n' + ',\n'.join(tla_event(e) for e in events) + '\n>>'

def spec(name, traces):
    text = '---------------- MODULE ' + name + ' ----------------\nEXTENDS TraceHarness\n'
    text += 'Trace == <<\n' + ',\n'.join(sequence(events) for events in traces) + '\n>>\n'
    text += '=============================================================================\n'
    OUT.mkdir(exist_ok=True)
    (OUT / (name + '.tla')).write_text(text)
    (OUT / (name + '.cfg')).write_text('SPECIFICATION TraceSpec\nCONSTANTS Scenario = "trace"\n Broken = "none"\n Traces <- Trace\nINVARIANT TraceAdmitted\nCHECK_DEADLOCK FALSE\n')

def tlc(name):
    out = OUT
    out.mkdir(exist_ok=True)
    command = ['java', '-XX:+UseSerialGC',
               '-XX:ActiveProcessorCount=1', '-Xms32m', '-Xmx256m',
               '-cp', os.environ['TLA2TOOLS_JAR'],
               'tlc2.TLC', '-workers', '1', '-seed', '1', '-fp', '0',
               '-metadir', str(out / (name + '-states')), '-config', name + '.cfg', name + '.tla']
    t0 = time.monotonic()
    run = subprocess.run(command, cwd=OUT, text=True, stdout=subprocess.PIPE,
                         stderr=subprocess.STDOUT, timeout=55)
    (out / (name + '.log')).write_text(run.stdout)
    admitted = run.returncode == 0 and 'Model checking completed. No error has been found.' in run.stdout
    refuted = run.returncode == 12 and 'Invariant TraceAdmitted is violated' in run.stdout
    if not admitted and not refuted:
        raise RuntimeError(f'{name}: invalid TLC verdict/exit {run.returncode}; see {out / (name + ".log")}')
    return dict(admitted=admitted, refuted=refuted, exit=run.returncode,
                seconds=round(time.monotonic()-t0, 3), log='trace/results/' + name + '.log'), run.stdout

REFINEMENTS = {
    'seq-skip-race': 'WatcherRefresh disarms changed revision before second confirm; unchanged revision retains arm. Post-confirm CLI race remains model-only.',
    'preexisting-comment': 'MetadataRefused: unreadable MERGE_MSG fails closed before staging; readable route later resumes. No pure-C5 end-to-end fail-open.',
}
CONFIRMED_PAIRS = [
    dict(check='draft-io-quit', trials=['LeaveIO'], strength='runtime consequence confirmed',
         evidence='DrillLeaveResume.md:7-24; autosave error + empty draft directory + restart screenshot with missing manual marker; unchanged Git is not draft durability evidence'),
    dict(check='preserve-abort', trials=['seq-abort-loss'], strength='runtime consequence confirmed; failure before write, not model truncation',
         evidence='DrillSequencers.md:15-25; chmod 000 + B/A screenshot + successful Abort/changed Git + retained buffer only A, recoveries empty'),
    dict(check='stale-worktree', trials=['LeaveExternalWT'], strength='runtime consequence confirmed',
         evidence='DrillLeaveResume.md:43-65; external blob b90a253a1cbd4b9a5296eacbd561b0cb5245118a replaced by stage-0/WT fa1707ed6578871525bc9ab3cf5f7e7a5a5326ab'),
    dict(check='draft-lost-update', trials=['LeaveTwoA', 'LeaveTwoB'], strength='mechanism-level only; NOT exact distinct-path model trace',
         evidence='DrillLeaveResume.md:26-41; same-file distinct-hunk overwrite, screenshots and shared buffer.json; no torn JSON or distinct-path race witnessed'),
]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--extract', action='store_true')
    parser.add_argument('--trials', type=Path)
    args = parser.parse_args()
    if args.extract and args.trials is None:
        parser.error('--extract requires --trials PATH; no external trial path is assumed')
    t0 = time.monotonic()
    trials = extract(args.trials) if args.extract else json.loads((ROOT / 'events.json').read_text())
    if not trials or any(not t['events'] for t in trials):
        raise ValueError('Saved trace projection must contain nonempty trials/events')
    OUT.mkdir(exist_ok=True)
    # Keep generated TLC inputs self-contained: module resolution must not depend
    # on the caller's working directory or a Java library-path interpretation.
    for module in (ROOT / 'TraceHarness.tla', ROOT.parent / 'ConflictLifecycle.tla'):
        (OUT / module.name).write_text(module.read_text())
    for n, trial in enumerate(trials, 1):
        spec('Trace' + str(n), [trial['events']])
    spec('AllTraces', [t['events'] for t in trials])
    batch, output = tlc('AllTraces')
    # A forged skip without arm must be rejected: catches a vacuous checker.
    spec('RejectedSkip', [[event('Start'), event('Detect', 'rebase'), event('ExecuteSkip', 'rebase')]])
    negative, neg_output = tlc('RejectedSkip')
    rows = []
    for n, trial in enumerate(trials, 1):
        individual = batch if batch['admitted'] else tlc('Trace' + str(n))[0]
        rows.append(dict(trial=trial['trial'], events_mapped=len(trial['events']),
                         klog_events=sum('raw' in e for e in trial['events']),
                         witness_events=sum(e['kind'].startswith('Witness') or e['kind']=='ExternalObserved' for e in trial['events']),
                         admitted=individual['admitted'], admission_scope='runtime observable quotient; not a complete hidden-state lift',
                         gap_type=('refined model gap + log gap' if trial['trial'] in REFINEMENTS else 'log gap'),
                         refinement=REFINEMENTS.get(trial['trial'], 'Runtime projection: repeated detection, UI routing, aggregate receipts and separate Git-witness lane; hidden steps remain unobservable.'),
                         unobservable_steps=trial['gaps'],
                         unmapped=trial['unmapped'], log_files=trial['log_files'],
                         spec='trace/results/Trace' + str(n) + '.tla', sources=trial['sources']))
    summary = dict(seconds=round(time.monotonic()-t0, 3), trials=len(rows),
                   logs=sum(t['log_files'] for t in rows), batch=batch, negative_control=negative,
                   ok=all(r['admitted'] and not r['unmapped'] for r in rows) and negative['refuted'],
                   confirmed_pairs=CONFIRMED_PAIRS,
                   not_witnessed=['skip-confirm-cli-race', 'draft-torn', 'draft-two-writers distinct-path tear',
                                  'receipt-close race', 'stale-after-preflight', 'C6 zero-exit/no-movement'],
                   results=rows)
    (OUT / 'summary.json').write_text(json.dumps(summary, ensure_ascii=False, indent=2) + '\n')
    for row in rows:
        print(f"{'PASS' if row['admitted'] else 'FAIL'} {row['trial']}: {row['events_mapped']} events; log gaps remain")
    print(f"Trace checks: {len(rows)} trials / {summary['logs']} logs; {summary['seconds']}s; forged Skip={'REFUTE' if negative['refuted'] else 'ERROR'}")
    if not batch['admitted']:
        print(output[-7000:])
    if not negative['refuted']:
        print(neg_output[-7000:])
    return 0 if summary['ok'] else 1

if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as exc:
        print(f'TOOL ERROR: {exc}', file=sys.stderr)
        raise SystemExit(2)
