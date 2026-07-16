# mecojoni-core

`mecojoni-core` is the dependency-free, `#![no_std] + alloc`, unsafe-free v2
compiler and runtime. Hosts own files, import resolution, seeds, typed data,
localized formatter resources, persistence, clocks, and concurrency ordering.

Source compilation constructs one private immutable `lowered-ir/1` grammar.
Every construction path validates rule indexes, entries, production references,
and cached static selections before generation can observe it. Public artifact
policy types define bounded `full`, `mapped`, and `stripped` profiles for the
experimental `bytecode/0` implementation.

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
preloaded `Formatter`. See the root README, generated `cargo doc`,
`COMPATIBILITY.md`, and `CONFORMANCE.md` for the full contracts.
