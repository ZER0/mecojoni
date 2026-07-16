# Mecojoni v2 conformance suite

The source specification is backed by parser-independent files and cross-runtime
contracts rather than snapshots of implementation internals.

| Contract | Location | Proof |
| --- | --- | --- |
| Canonical syntax corpus | `README.md` plus `crates/mecojoni-core/tests/fixtures/expected/readme-corpus.ast` | Parsed AST prediction and exact generation tests. |
| Invalid headers/bodies | `crates/mecojoni-core/tests/fixtures/invalid/` | Stable codes and exact byte/scalar spans. |
| Package/import semantics | `crates/mecojoni-core/tests/fixtures/packages/` | Filesystem modules, imports, visibility, typing, messages, diversity, and replay. |
| Fluent formatter composition | `crates/mecojoni-core/tests/fluent_integration.rs` plus `crates/mecojoni-core/tests/fixtures/packages/fluent/` | Real `.ftl` resources receive typed Mecojoni arguments and prove English/Polish plurals, gender selects, explicit fallback, provenance, and default bidi isolation. |
| Numeric/PRNG/profile vectors | `crates/mecojoni-core/tests/fixtures/expected/` | Exact cross-runtime values and policy constants. |
| CLI process contract | `crates/mecojoni-cli/tests/` | Every command, stream, JSONL record, flag form, and status. |
| v1 migration | `crates/mecojoni-cli/tests/fixtures/v1/` | Frozen reader, hazard diagnostics, compilable rewrite, honest output-set comparison. |
| Workload operations | `crates/mecojoni-benchmarks/baselines/` | Native/WASM source, artifact, expansion, sampler, text, and leak checks. |
| JavaScript/WASM | `js/*_test.ts` | The identical artifact through Deno and headless Chrome. |

Run the complete local gate:

```sh
cargo +1.85.0 fmt --all -- --check
cargo +1.85.0 test --workspace --all-targets
cargo +1.85.0 test --workspace --doc
cargo +1.85.0 clippy --workspace --all-targets -- -D warnings
cargo +1.85.0 doc --workspace --no-deps
cargo +1.85.0 check -p mecojoni-core --target thumbv6m-none-eabi
cargo +1.85.0 build -p mecojoni-wasm --target wasm32-unknown-unknown --release
deno task js:check
deno task wasm:test
deno task wasm:browser:test
```

The core filesystem suite also applies deterministic byte/scalar mutations to
valid source and verifies that malformed UTF-8 or syntax always returns a value
or structured diagnostic rather than panicking.
