# Mecojoni v2 benchmark contract

`workloads/1` is a committed, code-generated suite shared by native Rust and
Deno/WASM. It separates realistic and adversarial shapes:

| Scenario | Class | Shape |
| --- | --- | --- |
| `flat-64` | realistic | One ordinary flat choice rule. |
| `tree-dialogue` | realistic | A sentence assembled from four semantic leaves. |
| `chain-512` | adversarial | A deep linear graph using explicit benchmark limits. |
| `dense-dag-96x8` | adversarial | Ninety-six rules with up to eight forward edges each. |
| `recursive-balanced` | realistic | Termination-biased recursive balanced text. |
| `fanout-10000` | adversarial | One rule with ten thousand unique alternatives. |

Run the five-sample native release harness:

```sh
cargo +1.85.0 run -p mecojoni-benchmarks --release
```

It reports median wall time, exact allocation calls/bytes from a benchmark-only
counting system allocator, retained live bytes, and deterministic expansions,
sampler words, and output bytes. The benchmark crate is the only native crate
that permits the unsafe `GlobalAlloc` adapter; `mecojoni-core` remains
`forbid(unsafe_code)`.

Run the equivalent release WASM harness in Deno:

```sh
deno task wasm:bench
```

It reports compile/generation time, linear-memory pages before/after compile and
dispose, operation counts, live handles, and host-visible ABI allocations. The
normative Deno test requires zero leaked handles/allocations and at most one page
of linear-memory growth after warm generation.

## Cross-platform gate

[`operations-v1.contract`](crates/mecojoni-benchmarks/baselines/operations-v1.contract)
freezes source size, rule/production counts, artifact hash, seed-zero text,
expansions, and sampler words. Rust filesystem integration and Deno/WASM tests
must match it exactly. This zero-tolerance operation gate is meaningful across
hardware; wall time is not.

Native allocation counts and timings are deterministic or low-noise on a fixed
toolchain/machine but remain evidence, not a universal product promise. Shared CI
runners therefore do not fail on absolute latency. The WASM memory regression
gate is structural: no live ABI allocations/handles and no more than 64 KiB warm
growth.

## Optimization evidence

The first 2026-07-16 release run on arm64 Darwin, Rust 1.85.0, before retained
optimizations measured the `fanout-10000` workload at 326 ms compile time and
22.7 ms / 168,909,683 allocated bytes for 100 generations. It identified two
specific costs:

1. trace-off static selection rebuilt and normalized all 10,000 eligible weights
   on every call;
2. production-ID collision validation scanned all preceding alternatives.

The retained changes precompute cumulative static weights and use binary search
without changing the random draw, and replace the collision scan with an ordered
map. A five-sample release run after both changes measured:

| `fanout-10000` metric | Before | After median | Change |
| --- | ---: | ---: | ---: |
| Compile time | 326 ms | 234 ms | −28% |
| Generation time, 100 calls | 22.7 ms | 0.049 ms | −99.8% |
| Generation allocated bytes, 100 calls | 168,909,683 | 64,883 | −99.96% |
| Compile allocated bytes | 29,061,702 | 30,430,534 | +4.7% |

The compile allocation increase is the persistent cumulative-weight index plus
the temporary ordered collision map. It is retained because it removes repeated
runtime work and improves compile asymptotics. Seed mapping is proven identical
between cached trace-off selection and the full traced path for a 1,024-way rule.

No other optimization was retained: the remaining workloads did not justify an
alias table, bytecode, serialized IR, or a more complex history representation.
