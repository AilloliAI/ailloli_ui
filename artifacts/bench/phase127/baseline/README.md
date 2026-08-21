# Phase 127 baseline status

Status: **N/A — no comparable indexed pre-correction run exists.**

The first Phase 127 instrumentation and retained-tree contracts were introduced
after Internal commit `2b410c0`. No run with the final schema, the required
three excluded warmups, 30 measured samples, frozen machine metadata, and
correctness counters was captured at that revision before implementation began.

Backporting the final harness would also backport APIs that are themselves part
of the change (`Invalidation`, retained `TreeModel`, diagnostics, and bounded
virtual layout), so it would not be an honest before/after measurement. The
candidate runs next to this file are therefore the first indexed reference for
future comparisons. No zero-valued or reconstructed baseline is claimed.
