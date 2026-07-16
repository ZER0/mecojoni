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

Run the archived v1 JavaScript timing baseline in the same Deno/V8 host:

```sh
deno task v1:bench
```

The complete first cross-runtime result is checked in as
[`benchmarks/results/2026-07-16-darwin-arm64.json`](benchmarks/results/2026-07-16-darwin-arm64.json).
The file includes all six generated shapes plus the manually authored,
multi-module Harbor package under [`benchmarks/packages/harbor`](benchmarks/packages/harbor).
The filename `operations-v1.contract` below means version 1 of the operation
contract; it is not a benchmark of the Mecojoni v1 language.

The first source-versus-`bytecode/0` Harbor result is recorded separately as
[`benchmarks/results/2026-07-16-bytecode0-darwin-arm64.json`](benchmarks/results/2026-07-16-bytecode0-darwin-arm64.json).
On this run, native owned decode was 19.0 µs versus 66.5 µs source compilation;
Deno/WASM decode was 0.113 ms versus 0.261 ms source compilation. First
generation remained effectively unchanged. The full artifact is 7,407 bytes
versus 1,286 source-plus-manifest bytes before compression, so deployment-size
and embedded-WASM gates still decide B6.

The release generic WASM is 599,455 bytes and the Harbor-bearing WASM is 606,690
bytes. At gzip-9 they are 190,023 and 192,086 bytes; at Brotli-11 they are
151,040 and 153,024 bytes. Comparing the content WASM with the generic WASM plus
the separately compressed 1,286-byte source/manifest baseline gives a 0.62%
gzip or 0.84% Brotli increase, within the 20% budget. This is a conservative
embedded-source size proxy rather than a second executable source-embedding
pipeline. Chrome observed exactly one content-WASM request and no `.meco` or
`.mecob` request.

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

## Bytecode deployment budget

The first consumer needs a build-coupled, content-specific browser artifact: one
content-bearing `.wasm`, no runtime `.meco` fetches, and no browser-side import
resolution. Portable separately distributed `.mecob` files remain useful for
tooling but are not the primary deployment requirement.

The experimental format may freeze as `bytecode/1` only when the Harbor package
meets all of these gates on the same host/toolchain:

- bytecode load plus first output is at least 25% faster, or at least 1 ms faster,
  than source compile plus first output;
- the compressed content-bearing WASM is no more than 20% larger than the best
  embedded-source alternative;
- source, external-bytecode, and embedded-bytecode generation are exactly
  equivalent, including replay identity and traces; and
- repeated WASM load/generate/dispose cycles leak no handles or host allocations.

Generated stress shapes are recorded separately and can motivate engineering,
but cannot by themselves justify a permanent compatibility format.
