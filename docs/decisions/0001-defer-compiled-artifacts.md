# ADR 0001: keep source packages as the v2 distribution format

- Status: accepted for v2
- Date: 2026-07-16

## Decision

V2 does not define serialized compiled grammar artifacts. Hosts compile immutable
source packages once and reuse `CompiledGrammar`. Artifact hashes remain available
for caches and replay validation, but a cache miss recompiles trusted source.

## Evidence

The committed realistic workloads compile quickly after warm startup. The
adversarial 10,000-alternative single rule is slower (roughly 234 ms median native
and about one second in the measured Deno/WASM run), but it is also a hierarchy
lint target and not evidence that every package should acquire a second binary
format. Runtime selection for that workload was fixed independently without
serialization.

A compiled format would need its own schema version, bounds validation, endianness
and integer rules, diagnostic/source mapping, security review, cache invalidation,
and migrations for language, sampler, production-ID, and formatter-manifest
changes. No measured realistic startup requirement currently pays for that cost.

## Revisit when

A committed real package repeatedly misses a documented startup budget after
compile-pass profiling, or a distribution environment cannot ship source. Any
future format must be optional, content-addressed, bounded, fail closed, and prove
semantic equivalence against `workloads/1` and the full conformance corpus.
