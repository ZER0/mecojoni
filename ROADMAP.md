# Mecojoni v2 Roadmap

This roadmap turns the language in `README.md` and the runtime design in
`V2_SPECIFICATION.md` into testable implementation milestones. The README is the
authoritative syntax source. A syntax change is incomplete until the README,
specification, fixtures, and parser tests agree.

The original source-language milestones below are complete. The active compiled
artifact milestones B0–B6, including their exit gates, are defined in
[`BYTECODE_FORMAT_PLAN.md`](BYTECODE_FORMAT_PLAN.md). B0 is complete: the
repository records archived v1 JS, v2 native Rust, and v2 WASM/Deno evidence plus
a manually authored multi-module package and a single-WASM deployment budget.
B1–B6 are now also complete, and the evidence-backed result is the frozen
[`bytecode/1` format](BYTECODE_FORMAT.md).

## Fixed implementation constraints

- Rust is the primary implementation language and public API.
- `mecojoni-core` is `#![no_std]` plus `alloc`.
- The core starts with no third-party dependencies and contains no unsafe Rust.
- JavaScript targets `wasm32-unknown-unknown` through a handwritten ABI and thin
  JavaScript/TypeScript wrapper for Deno and browsers.
- The WASM adapter may provide a global allocator and contain narrowly reviewed
  ABI-level unsafe code.
- A C API is out of scope.
- Initial identifiers are case-sensitive ASCII; terminal text is unrestricted
  valid UTF-8. Unicode identifiers are a future versioned extension.
- Unit-test and integration-test crates may use `std`.
- Integration tests load real `.meco` packages and imports from checked-in
  filesystem fixtures.
- Seeded behavior, error codes, source spans, and ABI behavior are compatibility
  contracts once stabilized.

## Milestone 0 — Freeze the implementable contracts

Translate the prose design into artifacts an implementation can test.

- [x] Publish normative lexical rules and EBNF matching the README corpus.
- [x] Specify strict front-matter fields and indentation rules without depending
  on a YAML implementation.
- [x] Specify ASCII identifiers, qualified names, reference boundaries, comments,
  string escapes, raw strings, block chomping, weights, and empty output.
- [x] Specify guards, bindings, captures, typed parameters, `<-` argument punning,
  and complete-message effects.
- [x] Define source span coordinates, diagnostic structure, and initial stable
  error-code families.
- [x] Define the exact numeric representation and accepted syntax for weights.
- [x] Select and specify the deterministic PRNG and seed-to-state algorithm.
- [x] Specify and fixture the `location/1` diverse profile, `interactive/1`
  resource profile, and their replay-visible fixed-point/tokenizer details.
- [x] Specify and fixture the `composition/1` audit heuristic and its
  message-body exemption.
- [x] Specify and fixture text/JSONL CLI streams, exit statuses, and
  warning-failure policy.
- [x] Define host-supplied package loading and import resolution without core I/O.
- [x] Draft the versioned handwritten WASM ABI, ownership rules, handle lifecycle,
  allocator boundary, and JavaScript error model.
- [x] Record the minimum supported Rust version and edition.
- [x] Create parser-independent valid and invalid conformance fixtures.

**Exit gate:** every syntax example in the README has one predicted AST and every
invalid fixture has an expected error code and precise span.

## Milestone 1 — Establish the portable workspace

Create the smallest buildable skeleton before implementing language behavior.

- [x] Create a Cargo workspace with `mecojoni-core` and `mecojoni-wasm` crates.
- [x] Configure `mecojoni-core` as `no_std + alloc` with no default dependency on
  `std`.
- [x] Add compile checks for the native host and `wasm32-unknown-unknown`.
- [x] Deny unsafe code in the core crate.
- [x] Define foundational source ID, span, diagnostic, error, and result types.
- [x] Add unit-test support and a `std` integration-test fixture loader.
- [x] Establish `tests/fixtures/valid`, `invalid`, `packages`, and `expected`
  conventions.
- [x] Add CI gates for formatting, lints, unit tests, integration tests, and both
  compilation targets.

**Exit gate:** an empty core builds without `std`, the WASM target links with its
allocator, and an integration test reads a real fixture from disk.

## Milestone 2 — Lexer and parser

Implement source processing without generation shortcuts.

- [x] Validate UTF-8 and normalize physical line endings.
- [x] Lex front matter, headings, productions, weights, comments, sigils,
  bindings, guards, calls, strings, raw strings, and block literals.
- [x] Preserve exact byte and Unicode-scalar source coordinates.
- [x] Parse strict front matter without general YAML features.
- [x] Parse rules, typed parameter headings, productions, expressions, references,
  captures, bindings, message calls, and multiline argument lists.
- [x] Recover from independent syntax errors and aggregate diagnostics where safe.
- [x] Reject invalid ASCII identifiers while preserving arbitrary UTF-8 terminal
  text exactly.
- [x] Add unit tests for every token and AST form.
- [x] Add filesystem integration fixtures covering complete single-file sources,
  malformed inputs, CRLF, Unicode text, and exact diagnostics.

**Exit gate:** the full README corpus parses, canonical invalid fixtures fail with
their expected diagnostics, and parser code builds as `no_std + alloc`.

## Milestone 3 — Compiler and deterministic weighted generation

Deliver the first useful vertical slice through the Rust API.

- [x] Accept package sources and canonical module IDs supplied by the host.
- [x] Resolve modules, aliases, exports, optional default entry, rule references,
  and visibility.
- [x] Build immutable indexed IR without exposing mutable internal collections.
- [x] Validate duplicate and undefined names, rule arity, public entries, and
  malformed weights.
- [x] Compute reachability, productivity, nullable rules, recursive components,
  and recursion-risk diagnostics with iterative graph algorithms.
- [x] Implement exact bounded relative weights and unbiased `weighted/1` choice.
- [x] Implement the specified deterministic PRNG.
- [x] Expand with an explicit stack and exact depth, expansion, and output limits.
- [x] Support literal text, ordinary references, quoted/raw/block text, empty
  output, and productive recursion.
- [x] Expose compile and weighted-generation APIs from Rust.
- [x] Add deterministic seeded corpora, statistical weight checks, deep-recursion
  fixtures, and adversarial limit tests.

**Exit gate:** real multi-file fixtures compile from disk in integration tests;
fixed grammar/seed/request tuples produce fixed output and never consume the
native call stack recursively.

## Milestone 4 — First browser and Deno vertical slice

Prove the deployment model before the language grows further.

- [x] Finalize ABI version discovery and allocator/deallocator exports.
- [x] Expose package construction, compilation, weighted generation, result
  access, diagnostics, and handle disposal through opaque handles.
- [x] Prevent stale, double-freed, cross-kind, and out-of-range handle use.
- [x] Write the dependency-light JavaScript wrapper and TypeScript declarations.
- [x] Ensure JavaScript strings are encoded and decoded strictly at the boundary.
- [x] Add Deno integration tests using real `.meco` fixture files.
- [x] Add browser tests for the same compiled WASM artifact.
- [x] Verify repeated compile/generate/dispose cycles do not leak handles or linear
  memory beyond documented allocator behavior.

**Exit gate:** the same fixture and seed produce the same result through Rust,
Deno, and a browser, with equivalent structured diagnostics.

## Milestone 5 — Types, guards, parameters, captures, and bindings

Implement the v2 authoring features that solve v1's main composition limits.

- [x] Implement core scalar types, finite enums, immutable request data, and
  runtime type validation.
- [x] Implement typed rule declarations using `# rule <- name: type`.
- [x] Implement `@rule <- ...` calls, explicit named arguments, shorthand
  argument punning, arity checks, and type checks.
- [x] Implement restricted guard expressions and eligibility selection.
- [x] Implement `[weight = expression]` using immutable number inputs/parameters,
  bounded rational arithmetic, zero-weight ineligibility, and trace/replay data.
- [x] Reject dynamic-weight access to bindings, captures, rules, messages,
  callbacks, clocks, and ambient state.
- [x] Enforce guards before bindings and reject same-production binding use in a
  guard.
- [x] Implement emitting capture `@{rule as name}`.
- [x] Implement ordered non-emitting binding `{rule as name}`.
- [x] Enforce lexical scope, immutability, no shadowing, no forward references,
  and explicit parameter passing to child rules.
- [x] Make all bindings candidate-local and traceable.
- [x] Add integration fixtures for host data, enums, dynamic weights, multiple
  bindings, recursion frames, invalid calls, and deterministic binding order.

**Exit gate:** the guarded, parameterized, and binding-heavy examples embedded in
the README compile and generate exactly as documented through Rust and WASM.

## Milestone 6 — Complete-message localization boundary

Add localization without embedding a localization system in the core.

- [x] Implement stable `&message` references and typed `<-` arguments.
- [x] Define a synchronous host formatter request/result protocol.
- [x] Enforce the transitive complete-message effect.
- [x] Reject message capture, suffixing, wrapping, multiple visible messages, and
  message-valued non-emitting bindings.
- [x] Build and validate message/input manifests.
- [x] Support explicit locale and fallback information without ambient globals.
- [x] Add a test formatter for Rust integration tests and callback/protocol support
  in the WASM wrapper.
- [x] Add at least English and one locale with `few`/`many` categories to the
  integration corpus.

**Exit gate:** internally generated values feed a localized complete message in
Rust, Deno, and browser tests; missing messages and schema drift produce stable
diagnostics.

## Milestone 7 — Diverse sampling and transactional state

Rebuild repetition resistance with explicit ownership and rollback.

- [x] Implement versioned `diverse/1` semantics over documented base-weight
  priors.
- [x] Separate immutable grammar, sampler session, and repetition store.
- [x] Implement structural cooldown and bounded diversity factors.
- [x] Generate candidate-local state deltas and commit only the winner.
- [x] Add exact-output and opening/ending novelty histories with bounded storage
  and constant-time eviction.
- [x] Preserve recursion and nullable-rule probability contracts.
- [x] Define deterministic candidate work and PRNG consumption.
- [x] Reject overlapping mutation of one ordered session/store.
- [x] Add transactional rollback, cooldown, eviction, and deterministic replay
  integration tests.
- [x] Add golden tests for every `location/1` setting, including candidate count,
  cooldown relaxation, fragment windows, exact-history eviction, and nullable or
  recursive exemptions.

**Exit gate:** rejected, failed, cancelled, and over-budget candidates leave no
committed state, and a saved deterministic fixture sequence matches on every
supported target.

## Milestone 8 — Tracing, audits, and replay

Make generated text explainable and state reproducible.

- [x] Record stable rule, production, source, binding, message, and output-span
  provenance.
- [x] Attribute repeated visible fragments only to emitters whose spans overlap
  them.
- [x] Expose optional traces and metrics without paying their full cost when off.
- [x] Implement structural and rendered repetition audits.
- [x] Implement `composition/1` and fixtures for its direct-reference count,
  literal-run rule, and complete-message exemption.
- [x] Implement versioned session/repetition snapshots and replay receipts.
- [x] Define retention, logical-byte budgets, pinning, expiry, and sensitive-data
  handling.
- [x] Add round-trip replay tests after nonempty history through Rust and WASM.

**Exit gate:** awkward output can be traced to its actual visible emitters, audits
do not blame unrelated deep rules, and snapshot restore reproduces the next output.

## Milestone 9 — Authoring tools and v1 migration

Build tools only after the core contracts are stable.

- [x] Add an optional `std` CLI crate for check, generate, trace, lint, manifest,
  audit, migrate, and bench workflows.
- [x] Keep filesystem and process behavior outside `mecojoni-core`.
- [x] Implement a source formatter proven not to alter output semantics.
- [x] Freeze a v1 reader and implement explicit v1-to-v2 migration.
- [x] Produce migration diagnostics for ambiguous whitespace, sigils, empty text,
  comments, and weight-looking prose.
- [x] Add subprocess tests and real v1/v2 corpus comparisons.
- [x] Test text versus JSONL output, stdout/stderr separation, all defined exit
  statuses, and warning-failure thresholds.
- [x] Add initial editor grammar and language-server support if demanded by real
  authoring use.

The checked-in TextMate grammar covers current lexical use. Semantic editor
diagnostics invoke `meco check`; no real authoring requirement yet justifies a
separate incremental LSP transport or a duplicate parser.

**Exit gate:** checked-in v1 projects migrate explicitly, generated differences
are reported honestly, and every CLI command has filesystem integration tests.

## Milestone 10 — Optimization and stabilization

Optimize only measured workloads and prepare a stable release.

- [x] Commit representative flat, tree, chain, dense, recursive, and large-fanout
  benchmarks.
- [x] Track operation counts and allocation behavior across native and WASM
  targets.
- [x] Optimize graph passes, selection indexes, histories, or serialization only
  when a committed workload justifies the complexity.
- [x] Decide whether compiled artifact serialization is needed.
- [x] Freeze language, sampler, ABI, snapshot, and diagnostic compatibility rules.
- [x] Publish Rust API documentation, JavaScript/TypeScript documentation,
  conformance fixtures, examples, and migration guidance.

`workloads/1` freezes six native/WASM operation contracts and measures native
allocations plus WASM linear memory, handles, and host-visible allocations. The
measured fan-out bottleneck justified a cumulative selection index and
`O(n log n)` production-ID validation; both preserve the frozen seed mapping.
The later B0–B6 evidence reopened and superseded the original deferral:
compiled-artifact serialization is implemented and frozen as
[`bytecode/1`](BYTECODE_FORMAT.md). The decision history remains in
[`docs/decisions/0001-defer-compiled-artifacts.md`](docs/decisions/0001-defer-compiled-artifacts.md).
See [`BENCHMARKS.md`](BENCHMARKS.md), [`COMPATIBILITY.md`](COMPATIBILITY.md),
[`CONFORMANCE.md`](CONFORMANCE.md), and [`RELEASE.md`](RELEASE.md) for the
evidence and stable-release gates.

**Exit gate:** every retained optimization has before/after evidence, all public
compatibility contracts are versioned, and the release gate in
`V2_SPECIFICATION.md` is satisfied.

## Deferred ideas

- Unicode identifiers and NFC normalization.
- Typed feature records for internally generated grammatical agreement.
- A standards-based localization adapter package.
- Serialized compiled artifacts and streaming generation; see the conditional
  [`BYTECODE_FORMAT_PLAN.md`](BYTECODE_FORMAT_PLAN.md).
- Persistent external repetition stores.
- Bindings for additional host languages.

Deferred items are not compatibility promises. Each requires its own concrete use
case, contract, fixtures, and versioning decision before entering a milestone.
