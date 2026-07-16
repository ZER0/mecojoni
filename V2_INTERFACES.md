# Mecojoni v2 Host Interface Contracts

This document freezes the host-facing boundaries that later roadmap milestones
implement. `README.md` remains authoritative for language syntax,
`V2_SYNTAX.md` formalizes parsing, and `V2_SPECIFICATION.md` owns runtime
semantics.

## Host-supplied packages (`package/1`)

The core performs no filesystem, URL, registry, or network resolution. A host
constructs one complete `PackageInput` before compilation:

```text
PackageInput
  root_id: canonical module ID
  modules[]:
    canonical_id: unique, nonempty host identity
    source: owned UTF-8 SourceFile
    resolved_imports[]:
      authored_path: exact path string from that source's front matter
      target_id: canonical ID of another supplied module
```

The root ID names exactly one supplied module. Canonical IDs and `SourceId`s are
package-local identities; source-declared `module` names remain language
namespaces and are validated separately. For every authored import path the host
supplies exactly one resolution edge, supplies no undeclared edge, and supplies
the target module in the same package. The compiler then associates the authored
alias with that target.

Hosts may canonicalize paths, enforce sandboxes, fetch modules, or read files
before constructing this value. None of that behavior enters core semantics.
Package content/replay hashes include canonical IDs, source bytes, the explicit
root, and sorted resolution edges, so a host resolution change is observable.

Initial format 2 rejects import cycles. Rule recursion within the resolved package
remains legal subject to graph analysis and runtime limits.

## Rust weighted API (`weighted/1`)

`compile_package(&PackageInput)` parses and resolves the complete package into a
private indexed `CompiledGrammar`. Public queries expose entry names, a default
entry, immutable rule-analysis facts, and compiler warnings without exposing
mutable rule or production collections. The executable subset includes exact
static/dynamic weights, typed scalar and enum data, guards, typed calls, captures,
ordered bindings, ordinary rule references, all literal/block forms, empty output,
productive recursion, and complete external messages. Message-bearing packages
use `compile_package_with_manifest(&PackageInput, &MessageManifest)`. Compilation
checks the portable ID profile and exact named argument types, computes the
transitive complete-message effect, and rejects captures, visible composition,
multiple messages, or message-valued silent bindings. `CompiledGrammar::manifest`
returns a deeply owned input/message schema for host serialization.

`CompiledGrammar::generate_weighted(&GenerationRequest)` is stateless. A request
contains a seed, an optional qualified public entry, an immutable `&[DataBinding]`,
optional binding- and selection-trace flags, and explicit depth, expansion, output-scalar,
output-byte, and sampler-word limits. `Value` currently owns text, an exact
`Rational`, a boolean, or a finite enum member. Root inputs use their bare name in
request data; imported-module inputs use `declared-module.input`. Missing, extra,
duplicate, wrongly typed, and unknown enum values are rejected before sampling.
Omitting the entry uses the root default or returns `E_NO_ENTRY`. Each rule selection—including a
single-production rule—uses unbiased `splitmix64/1` rejection sampling and
therefore consumes at least one PRNG word. Expansion uses heap frames with a body
cursor, so native call-stack depth and production width do not define language
limits. `GenerationResult` returns text, the resolved entry, and exact expansion
and sampler-word counters plus optional ordered `BindingTrace` values.

`generate_weighted_structural(request, locale)` stops at either ordinary text or
one owned `FormatterRequest { message_id, arguments, requested_locale,
fallback_locales }`. `generate_weighted_with_formatter` synchronously passes that
request to a side-effect-free `Formatter` over already-loaded resources and then
validates the complete response. `FormatterResult` contains text, actual locale,
environment hash, diagnostics, work units, and a replayability flag. Actual locale
must occur in the explicit request chain; formatter work is capped at 10,000; a
replayable result requires a nonempty environment identity; fatal formatter
diagnostics and final UTF-8/scalar limit failures abort without partial text.
Successful `GenerationResult` values retain formatter warnings and coarse
`MessageTrace` provenance.

```rust
let data = [
    DataBinding::new("playerName".into(), Value::Text("Rin".into())),
    DataBinding::new("mood".into(), Value::Enum("tense".into())),
    DataBinding::new("urgency".into(), Value::Number(Rational::new(2, 1)?)),
];
let result = grammar.generate_weighted(&GenerationRequest {
    data: &data,
    trace_bindings: true,
    trace_selections: true,
    ..GenerationRequest::with_seed(7)
})?;
```

The TypeScript equivalent passes discriminated values so no JavaScript number is
silently rounded:

```ts
meco.generateWeighted(grammar, {
  seed: 7n,
  data: {
    playerName: { kind: "text", value: "Rin" },
    mood: { kind: "enum", value: "tense" },
    urgency: { kind: "number", numerator: 2n, denominator: 1n },
  },
  traceBindings: true,
  traceSelections: true,
});
```

## WebAssembly ABI (`meco-wasm/1`)

The adapter targets `wasm32-unknown-unknown`, exports synchronous C-shaped
functions, and imports no WASI facilities. All integers are little-endian. ABI
version discovery is always safe before allocation:

```text
u32 meco_abi_version()       // 1
u32 meco_core_api_version()
```

### Linear-memory ownership

The final adapter exports:

```text
u32  meco_alloc(u32 length, u32 alignment)
void meco_dealloc(u32 pointer, u32 length, u32 alignment)
```

`0` is the null/failure pointer. Alignment is a nonzero power of two no greater
than 64. JavaScript writes input only inside its live allocation and transfers
temporary ownership to one synchronous `meco_call`; the call borrows but does not
retain that range. JavaScript deallocates it afterward. Output stays adapter-owned
behind a result handle until disposal and is copied through bounded accessors.
Invalid ranges, overflow, misalignment, allocation failure, or double-free are
structured ABI errors or traps only when the caller violates the documented raw
allocator preconditions. Ergonomic wrapper APIs never expose raw pointers.

The adapter uses `dlmalloc` only on `wasm32`; the safe core remains allocator-
agnostic and dependency-free.

### Calls, wire payloads, and handles

One versioned dispatch surface prevents an export explosion:

```text
u32 meco_call(u32 operation, u32 input_pointer, u32 input_length)
```

It always returns a nonzero result handle when the raw input range is valid and a
handle ID remains available, including for ordinary language/runtime errors. A
zero return means the input range was outside a live `meco_alloc` allocation or
the monotonic handle space was exhausted. Operation IDs and request payloads use
`meco-wire/1`: fixed little-endian integers and length-prefixed strict UTF-8 byte
strings. No Rust layout, pointer, enum discriminant, JSON number, or JavaScript
object layout crosses the ABI.

Every request begins with `u32 wire_version = 1`. `str` below is `u32 byte_length`
followed by strict UTF-8 bytes; `opt_str` is a one-byte `0`, or `1` followed by a
`str`. Counts and payloads are bounded before allocation, duplicate source IDs
are rejected, and trailing bytes are errors.

| Operation | ID | Request after version | Success value |
| --- | ---: | --- | --- |
| package create | 1 | `str root`, module count; each module has canonical ID, source ID/name/bytes, and resolved import pairs | package handle |
| compile | 2 | package handle | grammar handle plus entries/default/warnings payload |
| weighted generate (legacy scalar-free request) | 3 | grammar handle, `u64` seed, optional entry, five `u32` limits | text/entry/work-counter payload |
| typed weighted generate | 4 | operation-3 fields plus trace flags and typed request-value map | text/entry/work-counter, binding-trace, and selection-trace payload |
| compile with message manifest | 5 | package handle plus message count; each message has ID and ordered named schema types | grammar handle plus entries/default/warnings payload |
| structural typed generate | 6 | operation-4 fields plus requested locale and ordered fallback strings | ordinary text or one typed formatter request, then entry/work counters and traces |
| repetition store create | 7 | no fields | `location/1` repetition-store handle |
| sampler session create | 8 | `u64` seed | `diverse/1` sampler-session handle |
| diverse generate | 9 | operation-4 fields with reserved seed zero, followed by session/repetition handles and cancellation flag | text/traces plus attempts, winner score, and committed revision |
| session snapshot export | 10 | session handle | `snapshot/1` bytes |
| session snapshot import | 11 | bounded `snapshot/1` bytes | restored session handle plus canonical bytes |
| repetition snapshot export | 12 | repetition-store handle | `snapshot/1` bytes |
| repetition snapshot import | 13 | bounded `snapshot/1` bytes | restored repetition-store handle plus canonical bytes |

Generation limits are depth, expansions, output Unicode scalars, output UTF-8
bytes, and sampler words in that order. A `u64` is always little-endian and the
TypeScript API accepts it as `bigint`.

Operation 3 remains byte-for-byte compatible with the first ABI-1 vertical slice;
operation 4 is the additive typed extension used by message-free requests.
Operation 5 compiles message-bearing packages without changing operation 2.
Operation 6 produces structure for a synchronous host callback without WASM
imports, host re-entry, or locale I/O inside the module. Request
values use a one-byte kind: text `0` plus `str`; number `1` plus signed
two's-complement `i64` numerator and positive `u64` denominator; boolean `2` plus
`0` or `1`; enum `3` plus its member `str`. The request map is canonically sorted
by the TypeScript wrapper. Generation binding traces encode name, emitted flag,
and the same typed value form. Selection traces encode the qualified rule, winner,
and every eligible production's exact rational plus normalized integer weight.

Result access is read-only and bounded:

```text
u32 meco_result_status(u32 result)             // success or structured error
u32 meco_result_value_handle(u32 result)       // 0 when absent
u32 meco_result_payload_length(u32 result)
u32 meco_result_payload_copy(u32 result, u32 destination, u32 capacity)
void meco_handle_dispose(u32 handle)
u32 meco_live_handle_count()
```

Payload copy returns the required length without writing when capacity is too
small. Calling `meco_result_value_handle` claims the returned value handle and
transfers its disposal responsibility to the caller. Disposing a success result
before claiming its value also disposes that still-owned value, so ignored results
cannot leak package or grammar handles. A value handle has an internal kind
(package builder, compiled grammar,
session, repetition store, result, snapshot, or replay bundle). Every operation
checks kind and liveness.

Handle `0` is invalid. Public handles are monotonically allocated `u32` values and
are never reused during one instance lifetime. Disposal removes the value and is
idempotent, so a repeated or unknown disposal cannot double-free. Using a stale,
unknown, or cross-kind handle in an operation returns a structured `E_ABI_*`
diagnostic; result accessors report invalid status `2` or zero. The live-handle
counter supports lifecycle tests and host leak telemetry. Dropping the WebAssembly
instance releases all remaining handles and allocator state.

Response payloads begin with `wire_version` and a kind: error `0`, package `1`,
compile `2`, generation `3`, or structural generation `4`. Diagnostics encode
code, severity, optional source
ID plus byte/scalar span as `u64`s, and message. Compile success encodes entries,
an optional default, and warnings. Generation success encodes text, resolved
entry, expansions, and sampler words. The wrapper claims any value handle and
copies the payload before disposing its result handle.

### JavaScript and Deno errors

The TypeScript wrapper turns every result payload into:

```ts
type MecoResult<T> =
  | { ok: true; value: T; diagnostics: Diagnostic[] }
  | { ok: false; error: MecoError; diagnostics: Diagnostic[] };
```

It uses fatal UTF-8 encode/decode behavior, deterministic `bigint` conversion for
64-bit fields, and `try/finally` disposal. Ordinary core failures remain result
values rather than unclassified JS exceptions. The wrapper rejects unpaired
UTF-16 surrogates before allocation. Deno is the normative JS integration host;
automated Chrome and in-app browser smoke tests load the identical `.wasm`
artifact through the same browser-neutral wrapper.

The wrapper's `compilePackage(description, manifest?)` selects operation 5 when a
manifest is present. `generateWeighted` selects operation 6 when `locale` and a
synchronous `formatter` callback are supplied. The wrapper invokes the callback
only after the WASM call has returned, validates the formatter result with the
same locale/work/provenance/output rules, and exposes message provenance plus
successful formatter diagnostics. A promise-like callback result is rejected;
applications preload catalogs before generation.

Operations 7–9 keep grammar, PRNG ownership, and repetition history distinct.
Operation 9 rejects wrong, stale, or already borrowed state handles, reserves the
fixed candidate substreams, and returns only after winner-only commit. The
TypeScript `generateDiverse` options intentionally contain no seed; the reserved
wire field is zero in ABI 1 and the session is the sole random source.

Typed generation requests carry binding, selection, and provenance trace flags in
that order. Selection records include stable production IDs as well as artifact-
local indexes. Provenance nodes encode parent, kind, qualified rule, stable
production ID, source span, optional byte/scalar output range, depth, and optional
binding/capture/message name. Diverse results append their versioned replay receipt.

Snapshot operations never expose mutable snapshot handles. Export returns owned
bytes; import validates version, kind magic, UTF-8, collection windows, logical-
byte declarations, and trailing-byte absence before creating a new opaque state
handle. TypeScript `snapshot()` and restore helpers use the same operations.

## CLI streams and statuses (`cli/1`)

The dependency-free optional `std` CLI implements this fixed matrix:

| Mode/condition | stdout | stderr | status |
| --- | --- | --- | ---: |
| `generate --output=text` success | each text followed by exactly one LF | requested traces/metrics | 0 |
| Human report command success | primary report | diagnostics | 0 |
| `--output=jsonl` success | one versioned JSON object per result/report | host/runtime notices only | 0 |
| Source/data/generation/formatter failure | no partial success record | diagnostics | 1 |
| Requested warning threshold reached | completed report where meaningful | warnings | 1 |
| Usage or host-I/O failure | none | concise usage/I/O diagnostic | 2 |
| Unexpected internal failure | none | internal-error diagnostic | 3 |

In JSONL mode traces are embedded in their result object and never interleaved on
stderr. Text generation is display output, not a lossless multiline record format.
Warnings do not change status unless an explicit threshold requests it. A command
never writes a partial generated value before formatter, output-limit, or state
commit success.

Commands accept both `--flag value` and `--flag=value`, never consume another
flag as a missing value, reject unknown/duplicate scalar flags, and write help to
stdout only when explicitly requested. `check`, `lint`, `generate`, `trace`,
`audit`, `manifest`, `migrate`, `fmt`, and `bench` follow this same contract.
Subprocess fixtures prove every row before `cli/1` is stable.
