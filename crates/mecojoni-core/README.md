# mecojoni-core

`mecojoni-core` is the dependency-free, `#![no_std] + alloc`, unsafe-free v2
compiler and runtime. Hosts own files, import resolution, seeds, typed data,
localized formatter resources, persistence, clocks, and concurrency ordering.

Source compilation constructs one private immutable `lowered-ir/1` grammar.
Every construction path validates rule indexes, entries, production references,
and cached static selections before generation can observe it. Public artifact
policy types define bounded `full`, `mapped`, and `stripped` profiles for the
frozen `bytecode/1` implementation.

`encode_artifact`, `decode_artifact`, `inspect_artifact`, and
`disassemble_artifact` provide the owned Rust artifact API. Decoding is bounded
by `ArtifactLimits`, checks the exact frozen runtime fingerprint and
content hash, reconstructs no source AST, and returns a grammar only after the
shared lowered verifier succeeds.

Artifact-loaded grammars preserve dynamic weights, guards, typed parameters,
ordered bindings and captures, complete message requests, source provenance,
composition audits, diverse sessions, replay receipts, and nonempty
session/repetition snapshots. The semantic package hash is unchanged, so a
snapshot created with a source-compiled grammar can continue against the decoded
artifact. `ArtifactMetadata::require_full_debug` provides a stable capability
failure for mapped and stripped artifacts.

```rust
use mecojoni_core::{
    GenerationRequest, PackageInput, PackageSource, SourceFile, SourceId,
    compile_package,
};

let source = "---\nmeco: 2\nmodule: hello\nentry: greeting\nexports: [greeting]\n---\n\n# greeting\n- Hello!\n";
let package = PackageInput {
    root_id: "hello".into(),
    modules: vec![PackageSource {
        canonical_id: "hello".into(),
        source: SourceFile::new(SourceId::new(0), "hello.meco", source),
        resolved_imports: vec![],
    }],
};
let grammar = compile_package(&package)?;
let result = grammar.generate_weighted(&GenerationRequest::with_seed(7))?;
assert_eq!(result.text(), "Hello!");
# Ok::<(), mecojoni_core::MecoError>(())
```

Compile once and reuse the immutable grammar. `SamplerSession` and
`RepetitionStore` are separate explicit state for transactional `diverse/1`.
Complete messages use `compile_package_with_manifest` and a synchronous
preloaded `Formatter`. The filesystem-backed `tests/fluent_integration.rs` proof
uses the real `fluent-bundle` crate as a dev dependency to cover typed arguments,
English and Polish plurals, gender selection, ordered fallback, provenance, and
bidi isolation without adding a production dependency. See the root README,
generated `cargo doc`, `COMPATIBILITY.md`, and `CONFORMANCE.md` for the full
contracts.
