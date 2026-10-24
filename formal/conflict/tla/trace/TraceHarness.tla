--------------------------- MODULE TraceHarness ---------------------------
EXTENDS ConflictLifecycle
CONSTANT Traces
VARIABLE trial, cursor, candidates
TraceInit == /\ trial \in DOMAIN Traces
             /\ cursor = 0
             /\ candidates = {RuntimeInit}
             /\ s = "unused: runtime observable quotient"
TraceNext == /\ cursor < Len(Traces[trial])
             /\ candidates' = UNION {RuntimeAdvance(r, Traces[trial][cursor+1]) : r \in candidates}
             /\ cursor' = cursor + 1
             /\ UNCHANGED <<trial, s>>
TraceSpec == TraceInit /\ [][TraceNext]_<<trial,cursor,candidates,s>>
\* Empty candidate sets are retained, NOT pruned as disabled actions/deadlocks.
\* Thus a rejected observation produces a real invariant violation, not a
\* vacuous success on the longest accepted prefix. Terminal stutter is allowed.
TraceAdmitted == candidates # {}
=============================================================================
