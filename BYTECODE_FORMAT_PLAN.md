# Mecojoni bytecode format plan

> **Status:** proposed, not implemented and not a compatibility promise.
>
> This plan does not reverse
> [ADR 0001](docs/decisions/0001-defer-compiled-artifacts.md). Source packages
> remain the v2 distribution format until a real package demonstrates a startup,
> memory, or deployment requirement that justifies bytecode. The first prototype
> must use `bytecode/0`; only the completed conformance and measurement gates below
> may freeze `bytecode/1`.

## Objective

Define a deterministic, bounded compiled representation of a complete Mecojoni
package that can be:

- produced by an offline `std` tool;
- decoded by `mecojoni-core` using only `no_std + alloc`;
- loaded by native Rust and the handwritten WASM ABI;
- embedded with the runtime in one content-specific `.wasm` file;
- proven semantically equivalent to compiling the original sources; and
- rejected safely when corrupt, oversized, unsupported, or incompatible.

The first implementation should remove parsing, name resolution, type checking,
graph analysis, and static-selection preparation from application startup. It
does not initially attempt zero-copy execution or turn Mecojoni into a general
virtual machine.

## Non-goals

- Bytecode is not an authoring format and has no handwritten syntax.
- It does not replace `.meco` source, the source compiler, or migration tooling.
- It does not serialize sampler sessions, repetition histories, or replay bundles;
  `snapshot/1` remains responsible for mutable state.
- It does not embed formatter implementations or translation catalogs. It stores
  only the grammar's message manifest and semantic message references.
- It does not hide content. Strings embedded in WASM can still be extracted.
- It does not accept native Rust layouts, raw pointers, `usize`, or enum
  discriminants as an interchange format.
- The first version does not promise streaming execution, memory mapping, or
  in-place patching.
- Format-1 Mecojoni files must be migrated to source format 2 before compilation.

## Recommended delivery model

Use two maturity levels:

1. `bytecode/0` is experimental and build-coupled. Its header contains an exact
   compiler/runtime fingerprint, and incompatibility requires rebuilding it.
2. `bytecode/1` is frozen only after canonical bytes, cross-version policy,
   hostile-input decoding, source equivalence, and target measurements pass.

An embedded build may require an exact runtime fingerprint even after
`bytecode/1`. A separately distributed artifact may be loaded only by runtimes
that explicitly advertise support for its bytecode major version and semantic
contracts.

## Architecture

```text
root.meco + imported modules + message manifest
                       |
                       v
            existing source compiler
                       |
                       v
              private CompiledGrammar
                       |
                       v
          canonical bytecode encoder (std or alloc)
                       |
                       v
                    .mecob
                 /            \
                v              v
       native bytecode API   WASM data segment
                |              |
                +------v-------+
                       |
           bounded bytecode decoder
                       |
                       v
              private CompiledGrammar
                       |
                       v
          existing weighted/diverse runtime
```

Generation must not know whether its immutable grammar came from source or
bytecode. Source compilation and bytecode loading must produce the same public
entries, default entry, message/input manifest, stable production IDs, warnings,
traces, output, operation counts, and replay identity.

## Identity and compatibility

Keep these values distinct:

| Identity | Purpose | Rule |
| --- | --- | --- |
| Semantic package hash | Replay and source/bytecode equivalence | Preserve the existing `CompiledGrammar::artifact_hash()` computed from canonical sources, resolutions, and message manifest. |
| Bytecode content hash | Cache key and accidental-corruption detection | Compute over canonical encoded bytes with its own hash field cleared. It is not an authenticity claim. |
| Runtime fingerprint | Experimental build coupling | Required by `bytecode/0`; optional metadata after `bytecode/1` is frozen. |
| Contract versions | Semantic compatibility | Record source, rational, production-ID, weighted/diverse sampler, normalizer, tokenizer, profile, and bytecode versions. |

A source-compiled grammar and its decoded bytecode must retain the same semantic
package hash. Replay receipts therefore remain valid regardless of how the
grammar was loaded. A bytecode re-encoding may have a different bytecode hash
only after an explicitly versioned container change.

The `full` profile can recompute the existing semantic hash from its retained
sources, resolution edges, and manifest. The `mapped` and `stripped` profiles
cannot independently reconstruct a source-text hash; for them the field is a
compiler-produced semantic identity protected against accidental corruption by
the bytecode content hash and against tampering only by host-owned distribution
authentication.

The initial dependency-free hash can follow the repository's versioned FNV-1a64
practice for identity and corruption detection. It must be documented as
non-cryptographic. Artifact authenticity and signatures belong to the host's
distribution system; the decoder still validates every byte as untrusted input.

## Container overview

Use the `.mecob` extension and a sectioned, little-endian container. All indexes
are unsigned `u32`; all offsets and total byte lengths are unsigned `u64` checked
before conversion to `usize`. Zero is an ordinary index unless a field explicitly
uses `u32::MAX` as `none`.

### Provisional fixed header

The exact layout remains provisional during `bytecode/0`:

```text
magic                    [u8; 4] = "MECB"
container_major          u16
container_minor          u16
header_bytes             u32
flags                    u32
total_bytes              u64
section_count            u32
source_language_version  u32 = 2
core_api_version         u32
reserved                 u32 = 0
semantic_package_hash    u64
bytecode_content_hash    u64
runtime_fingerprint      [u8; 16]
```

Rules:

- integers are little-endian;
- reserved bytes and unused flag bits must be zero;
- `total_bytes` must equal the supplied slice length;
- the header and directory must fit before any allocation;
- a major mismatch fails; a newer minor may be accepted only when every unknown
  section is explicitly optional;
- duplicate required sections, overlaps, gaps containing nonzero bytes, and
  trailing bytes fail;
- timestamps, absolute build paths, random salts, and host-specific metadata are
  forbidden from canonical output.

### Section directory

Each directory record contains:

```text
kind          u16
flags         u16  // bit 0 = required
record_bytes  u32  // zero only for variable/blob sections
offset        u64
length        u64
count         u32
reserved      u32 = 0
```

Sections appear in ascending kind order. Fixed-record sections declare their
record width. Variable data is referenced by validated `(offset, length)` ranges
or table indexes, never by native pointers.

### Planned sections

| Section | Required | Contents |
| --- | --- | --- |
| Contract manifest | Yes | Slash-versioned semantic contracts and build fingerprint. |
| Strings | Yes | Deduplicated UTF-8 index plus byte blob. |
| Sources | Profile-dependent | Source IDs, names, optional text, and scalar/byte mapping metadata. |
| Types and constants | Yes | Scalar/enum definitions, canonical values, and exact rationals. |
| Inputs | Yes | External input name and type indexes. |
| Messages | Yes | Stable IDs and ordered argument schemas. |
| Entries | Yes | Public entry names, rule indexes, and optional default entry. |
| Rules | Yes | Names, parameters, production ranges, analysis flags, spans, and message effect. |
| Productions | Yes | Stable IDs, weight/guard/body ranges, binding ranges, span, and diversity factor. |
| Bindings and arguments | Yes | Rule target, local slot, name, and compiled value operands. |
| Instructions | Yes | Fixed-width body, weight-expression, and guard instructions. |
| Static selections | Yes | Precomputed cumulative weights and totals. |
| Diagnostics | Yes | Compiler warnings with stable codes, severity, spans, and messages. |
| Debug metadata | Optional | Compiler version, original module IDs, comments, or inspection-only data. |

The message manifest is part of the semantic package identity. Formatter
catalogs, locale resources, and formatter state are not.

## Canonical encoding

Canonical output is necessary for caching, reproducible builds, tests, and
meaningful bytecode hashes.

- Normalize the package into canonical module order before assigning serialized
  indexes, or remap compiler indexes during encoding.
- Sort the string table by UTF-8 bytes and remove duplicates.
- Use a fixed section order, fixed record widths, and zero-filled reserved fields.
- Store exact reduced rational `(i64 numerator, u64 denominator)` pairs.
- Preserve authored production IDs verbatim and derived IDs under
  `production-fnv1a64/1`.
- Encode booleans as exactly `0` or `1`; reject other values.
- Store enum values as their declared type and member indexes, not host enum
  discriminants.
- Preserve semantic argument order where it is observable; otherwise use the
  compiler's published canonical ordering.
- Exclude timestamps, temporary paths, allocator capacities, and hash-map order.
- Define canonical source-name handling so checkout location cannot change bytes.
- Re-encoding a decoded canonical artifact must reproduce identical bytes.
- Reordering host-supplied modules without changing canonical identities or
  resolution edges must not change the artifact.

## Instruction representation

Begin with fixed-width 16-byte instructions because they are easy to bound,
index, inspect, mutate-test, and validate:

```text
opcode    u8
flags     u8
reserved  u16 = 0
a         u32
b         u32
c         u32
```

The operands are indexes, counts, slots, or forward branch targets defined by the
opcode. No instruction contains a byte pointer. A rule or production references
an instruction range as `(start, count)`, so an `END` opcode is not needed for
correctness.

The provisional opcode families below describe lowering work; numeric assignments
must not freeze until the `bytecode/0` verifier and disassembler exist.

### Body instructions

| Instruction | Meaning |
| --- | --- |
| `EMIT_LITERAL string` | Append one interned UTF-8 literal and provenance span. |
| `EMIT_VALUE value` | Append one input/local/constant value under existing text rules. |
| `CALL_RULE rule, args` | Push an iterative expansion frame with an argument range. |
| `CAPTURE_RULE rule, slot, name` | Expand once, emit it, and bind the exact value. |
| `CALL_MESSAGE message, args` | Produce the sole complete formatter request. |

Non-emitting bindings remain production metadata evaluated in authored order
before the body. This preserves candidate-local scope without inventing an
independent mutation instruction.

### Compiled value operands

Use a tagged fixed record:

```text
kind   u8   // input, local, text, number, boolean, enum
flags  u8
reserved u16 = 0
a      u32
b      u32
c      u64
```

Indexes select input/local slots, strings, enum definitions, or constant-table
entries. Numbers refer to exact rational constants; they are never IEEE-754.

### Weight-expression instructions

Use a small verified stack program:

- `PUSH_RATIONAL`
- `LOAD_INPUT`
- `LOAD_PARAMETER`
- `ADD_EXACT`
- `SUBTRACT_EXACT`
- `MULTIPLY_EXACT`
- `RETURN_NUMBER`

The verifier computes stack depth, requires one final number, and rejects access
to bindings, messages, clocks, callbacks, or ambient state. Existing
`rational/1` overflow and zero-weight semantics remain authoritative.

### Guard instructions

Use a typed verified stack program:

- `PUSH_CONSTANT`, `LOAD_INPUT`, `LOAD_PARAMETER`
- `IS`, `IS_NOT`, `LESS`, `LESS_OR_EQUAL`, `GREATER`, `GREATER_OR_EQUAL`
- `NOT`
- `JUMP_IF_FALSE`, `JUMP_IF_TRUE`, `JUMP`
- `RETURN_BOOLEAN`

Forward branches encode the source language's short-circuit order. The verifier
requires all control-flow paths to reach `RETURN_BOOLEAN` with one boolean and the
same stack shape. Backward branches are forbidden, keeping the bytecode
non-Turing-complete.

## Decoder and verifier

The decoder is a public security boundary even when an artifact is normally
embedded. Implement it in two non-recursive passes.

### Pass 1: structural validation

- Validate header, versions, declared size, directory arithmetic, section order,
  alignment policy, overlaps, and required sections.
- Enforce the caller's byte budget and hard implementation caps before allocating.
- Check every `count * record_bytes`, offset, and end with checked arithmetic.
- Validate UTF-8 and canonical string-table ordering.
- Reject duplicate sections, nonzero reserved fields, unknown required sections,
  and trailing data.

### Pass 2: semantic verification

- Validate every string, type, constant, source, rule, production, message, entry,
  argument, binding, diagnostic, and instruction index.
- Check every range is contained within exactly one appropriate section.
- Verify local-slot scope, call arity, argument types, message schemas, and
  complete-message effects.
- Verify rational reduction/bounds and static cumulative totals.
- Verify instruction operands, stack types, maximum stack depth, and branch
  targets.
- Recheck rule productivity, reachability facts, nullability, recursion flags, or
  a cheaper proof certificate; never trust flags without validation.
- Recompute the bytecode hash. Recompute the semantic package hash for `full`,
  and validate its presence and contract version for `mapped`/`stripped`.
- Construct `CompiledGrammar` only after the complete artifact succeeds; return no
  partial grammar on failure.

The first decoder may allocate owned `String` and `Vec` values matching the
current IR. It must not use unsafe code. A future borrowed or zero-copy decoder is
a separate optimization milestone with its own measurements and threat review.

## Limits

Start with a 64 MiB default and hard artifact-byte limit, matching the existing
snapshot ceiling, then adjust only from committed workloads. Retain the existing
WASM package limits of 4,096 modules, 4,096 imports per module, 1 MiB per ordinary
string, and 16 MiB per source. Add explicit independent limits for rules,
productions, constants, instructions, stack depth, diagnostics, source-map bytes,
and total decoded logical bytes.

Every public loader accepts an `ArtifactLimits` value but cannot exceed compiled
hard maxima. Limits are checked before allocation and again after decoding shared
tables. Limit failures use one stable diagnostic rather than allocator failure or
panic.

## Debug and source profiles

Support three explicit profiles without changing runtime semantics:

| Profile | Retained information | Intended use |
| --- | --- | --- |
| `full` | Original normalized sources, source names, byte/scalar spans, and warnings | Development, audits, and complete diagnostics. |
| `mapped` | Source names, compact span mapping, stable IDs, and warnings | Production with actionable diagnostics. |
| `stripped` | Stable rule/production/message IDs and minimum provenance only | Size-sensitive deployments that accept limited source diagnostics. |

The profile is header-visible and included in the bytecode content hash, not the
semantic package hash. Generation output and replay identity must match across all
three profiles. APIs that require unavailable source detail return a stable
capability diagnostic rather than fabricated spans.

## Rust API plan

Add an internal artifact module first, then expose a small owned API:

```rust
pub const BYTECODE_VERSION: &str = "bytecode/1";

pub struct ArtifactOptions {
    pub debug_profile: ArtifactDebugProfile,
}

pub struct ArtifactLimits {
    pub maximum_bytes: u64,
    pub maximum_decoded_bytes: u64,
    // Independent table and instruction limits.
}

pub fn encode_artifact(
    grammar: &CompiledGrammar,
    options: ArtifactOptions,
) -> MecoResult<Vec<u8>>;

pub fn decode_artifact(
    bytes: &[u8],
    limits: ArtifactLimits,
) -> MecoResult<CompiledGrammar>;

pub fn inspect_artifact(
    bytes: &[u8],
    limits: ArtifactLimits,
) -> MecoResult<ArtifactMetadata>;
```

The actual encoder may need compiler-only source metadata that is not currently
retained in `CompiledGrammar`. Prefer an internal `compile_to_artifact` pipeline
or a private compiler result over making mutable IR public.

Add stable diagnostics provisionally named:

- `E_BYTECODE_MAGIC`
- `E_BYTECODE_VERSION`
- `E_BYTECODE_CORRUPT`
- `E_BYTECODE_LIMIT`
- `E_BYTECODE_CONTRACT`
- `E_BYTECODE_CAPABILITY`

Freeze names and meanings only with `bytecode/1`.

## CLI plan

Extend `cli/1` additively:

```sh
meco compile-artifact root.meco \
  --manifest messages.manifest \
  --profile full \
  --output root.mecob

meco inspect-artifact root.mecob
meco verify-artifact root.mecob
meco generate-artifact root.mecob --seed 7
```

`compile-artifact` resolves the complete source package with the same filesystem
loader as `check`, compiles it, encodes canonical bytes, decodes those bytes, and
runs an equivalence sanity check before writing atomically. It never writes a
partial artifact after diagnostics.

`inspect-artifact` reports versions, hashes, profile, sizes, counts, entries,
inputs, and message schemas without executing grammar content. JSONL output uses
versioned integer/string fields and never serializes numbers through lossy JSON
floating point.

## WASM and TypeScript plan

Add ABI-1 operations rather than changing existing signatures:

| Provisional operation | Purpose |
| --- | --- |
| `OP_ARTIFACT_LOAD` | Decode supplied `.mecob` bytes into an ordinary grammar handle. |
| `OP_ARTIFACT_INSPECT` | Return bounded metadata without creating a grammar. |
| `OP_EMBEDDED_GRAMMAR_OPEN` | Decode the artifact compiled into this WASM module. |

The TypeScript wrapper adds:

```ts
meco.loadArtifact(bytes, limits?)
meco.inspectArtifact(bytes, limits?)
meco.openEmbeddedGrammar(limits?)
```

Supplied bytes follow existing `meco_alloc`/`meco_call` ownership. The embedded
operation reads a private static WASM data segment and returns the same grammar
handle type used by source compilation. Existing disposal, live-handle, and
allocation telemetry applies unchanged.

## Single-WASM embedding plan

After artifact loading is stable, add an opt-in `mecojoni-wasm` build step:

1. `meco compile-artifact` resolves and validates the package offline.
2. A build script copies the selected `.mecob` into `OUT_DIR`, records
   `rerun-if-changed`, and rejects missing or multiple defaults.
3. The WASM crate includes that exact file as a private static byte slice.
4. `OP_EMBEDDED_GRAMMAR_OPEN` decodes it on first application use.
5. The browser loads its application JavaScript and one content-bearing `.wasm`;
   it performs no `.meco` fetches or import resolution.

Keep ordinary generic WASM builds artifact-free. A content-specific build must be
explicit, reproducible, and report the embedded semantic and bytecode hashes.

Do not accept arbitrary filesystem paths through a Cargo feature. Use a dedicated
build command or generated thin application crate so build inputs are visible and
cache invalidation is correct.

## Verification matrix

### Semantic equivalence

- Compile every canonical, multi-module, typed, message-bearing, recursive, and
  diverse fixture from source and through bytecode.
- Compare entries, default entry, warnings, package manifest, rule analysis, and
  stable IDs.
- Compare exact text, expansions, sampler words, binding/selection/provenance
  traces, formatter requests, audits, and replay receipts over fixed seed corpora.
- Restore nonempty sampler/repetition snapshots against source- and
  bytecode-loaded grammars and reproduce the same next result.
- Prove `full`, `mapped`, and `stripped` profiles have identical generation.

### Canonical bytes

- Same package, toolchain contract, and profile produce identical bytes across
  repeated builds, native operating systems, and module input order.
- Decode then encode produces the exact original canonical bytes.
- Golden artifacts pin minimal, all-instruction, messages/imports, recursive, and
  large-fanout packages.
- Any source, resolution, or message-manifest change covered by the existing
  semantic hash changes that hash; encoding-profile and bytecode-only metadata
  changes alter only the bytecode hash.

### Hostile input

- Mutate every header and directory field.
- Truncate at every byte boundary for small golden artifacts.
- Exercise invalid UTF-8, duplicate strings, overlapping ranges, huge counts,
  integer overflow, unknown required sections/opcodes, nonzero reserved bits,
  invalid indexes, malformed rationals, stack under/overflow, and bad branches.
- Ensure every failure is a stable diagnostic with no panic, trap, leaked handle,
  or partial grammar.
- Run decoder fuzzing on native Rust and deterministic mutation corpora through
  Deno/WASM.

### Cross-target and embedding

- Load identical artifact bytes in native Rust, Deno, and Chrome.
- Verify exact operation contracts against source compilation.
- Verify embedded and externally supplied artifact paths produce identical
  grammar handles and output.
- Repeated load/generate/dispose cycles return WASM handles and host-visible
  allocations to zero.
- Verify the browser smoke test performs no `.meco` network requests.

### Performance gates

Record source versus bytecode for all six `workloads/1` scenarios and at least one
real multi-module application:

- artifact generation time;
- source and artifact bytes before and after HTTP gzip/Brotli;
- native and WASM load/compile time;
- peak and retained allocation bytes;
- WASM linear-memory pages before/after load and warm generation;
- first generation and steady-state generation time; and
- total browser fetch plus instantiation time for separate and embedded delivery.

Bytecode proceeds to `bytecode/1` only if a real target requirement improves
materially after accounting for larger download/instantiation cost. No absolute
latency promise is inferred from one machine.

## Implementation milestones

### Milestone B0 — Reopen the decision with evidence

- Add comparable recorded source-compile baselines for native Rust and WASM.
- Add at least one representative real package, not only generated workloads.
- State the startup, memory, or single-asset deployment budget bytecode must meet.
- Decide whether the first consumer needs a portable artifact or only a
  build-coupled embedded one.

**Exit:** a committed measurement and deployment requirement justifies work beyond
the embedded-source alternative.

### Milestone B1 — Stabilize the lowered IR boundary

- Document every current compiled rule, value, weight, guard, binding, body,
  message, analysis, and static-selection invariant.
- Refactor compiler and generator behind one private immutable grammar view.
- Add source-compiled semantic golden tests before encoding anything.
- Define artifact limits and debug profiles.

**Exit:** two internal grammar representations could drive generation without
exposing mutable compiler internals.

### Milestone B2 — Implement experimental `bytecode/0`

- Implement canonical section encoder, owned decoder, verifier, disassembler, and
  provisional diagnostics in `mecojoni-core`.
- Require an exact runtime fingerprint.
- Add golden files, round trips, corruption cases, and deterministic mutation
  integration tests.
- Prove source/bytecode equality for weighted generation first.

**Exit:** all source fixtures either produce equivalent bytecode execution or a
documented unsupported-feature diagnostic; malformed artifacts fail safely.

### Milestone B3 — Complete runtime semantics

- Cover dynamic weights, guards, parameters, bindings, captures, messages,
  provenance, audits, diverse sessions, snapshots, and replay.
- Add debug profile behavior and capability diagnostics.
- Run native, `thumbv6m-none-eabi`, and WASM checks.

**Exit:** the entire current v2 semantic corpus is source/bytecode equivalent.

### Milestone B4 — Add tooling and external WASM loading

- Add CLI compile, inspect, verify, and generate-artifact commands.
- Add ABI-1 artifact operations and TypeScript APIs.
- Test filesystem CLI processes plus Deno and Chrome artifact loading.
- Record complete native/WASM result files instead of only terminal output.

**Exit:** one `.mecob` file is reproducibly generated, inspected, and executed on
every supported runtime with no leaks.

### Milestone B5 — Add content-specific single-WASM builds

- Add the explicit build pipeline and embedded-open operation.
- Ensure the generic WASM artifact remains content-free.
- Prove the browser loads no separate `.meco` dependency.
- Compare embedded bytecode with embedded source and separately fetched source.

**Exit:** a browser application can ship one content-bearing WASM asset and obtain
the same grammar/replay identity as source compilation.

### Milestone B6 — Freeze or stop

- Review performance, download size, memory, decoder complexity, fuzz results,
  and compatibility costs.
- If justified, freeze the container, opcodes, limits, diagnostics, and
  compatibility policy as `bytecode/1` and update `COMPATIBILITY.md`.
- If not justified, retain `bytecode/0` as experimental tooling or remove it;
  continue shipping embedded source packages.

**Exit:** the project makes an evidence-backed compatibility decision rather than
allowing an experimental encoding to become permanent accidentally.

## Optional later optimization: frozen zero-copy grammar

Only after the owned decoder is measured should the runtime consider
`FrozenGrammar<'a>` backed directly by validated artifact sections. That would
require generation and audit code to consume a shared immutable grammar-view
trait, careful lifetime ownership behind WASM handles, alignment rules, and a new
proof that no validated reference can escape its artifact bytes.

This is not required for `bytecode/1`. Decoding to the existing owned
`CompiledGrammar` captures most parse/compile startup savings with much lower
implementation and unsafe-code risk.

## Decisions required before implementation

1. What real package and startup/deployment budget reopens ADR 0001?
2. Is independent artifact distribution required, or is exact build coupling
   acceptable?
3. Which debug profile must production support?
4. Is a non-cryptographic content hash sufficient when authenticity remains
   host-owned, or does the product require a reviewed cryptographic dependency?
5. Must formatter catalogs be embedded separately, or is only grammar content in
   scope?
6. What hard artifact and decoded-memory limits fit the smallest supported host?
7. Is canonical artifact identity required across compiler patch releases before
   `bytecode/1`, or only after it freezes?

Until those questions have measured answers, this document is an implementation
plan, not a promise that bytecode will replace source packages.
