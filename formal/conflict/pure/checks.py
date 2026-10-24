"""Deterministic finite-domain ledger harness; no product imports or writes.
Exit 0: observed HOLD/REFUTE equals every declared expectation; 1: mismatch;
2: CLI misuse. --broken mutations are expected to exit 1, never swallowed.
"""
import argparse
import json
from dataclasses import asdict, dataclass

@dataclass
class Property:
    predicate: str
    property: str
    expected: str
    rust: str
    classification: str
    existing_test: str
    declared: str
    implicit: str
    domain_size: int = 0
    violations: int = 0
    counterexample: object = None

    def check(self, condition, witness):
        self.domain_size += 1
        if not condition:
            self.violations += 1
            if self.counterexample is None:
                self.counterexample = witness

    def report(self):
        row = asdict(self)
        row['result'] = 'REFUTE' if self.violations else 'HOLD'
        row['method'] = 'Python exhaustive finite quotient/bounded enumeration'
        row['matched_expectation'] = row['result'] == self.expected
        return row


def cli(run, variants):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--self-check', action='store_true')
    parser.add_argument('--broken', choices=variants)
    args = parser.parse_args()
    rows = [p.report() for p in run(args.broken)]
    print(json.dumps({'broken': args.broken, 'properties': rows}, sort_keys=True))
    raise SystemExit(0 if all(p['matched_expectation'] for p in rows) else 1)


def prop(predicate, name, expected, rust, test='None found for this exact property', classification='CONTRACT', declared='', implicit=''):
    return Property(predicate, name, expected, rust, classification, test, declared, implicit)


def masks(n):
    from itertools import product
    return product((False, True), repeat=n)
