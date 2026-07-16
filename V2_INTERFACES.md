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

It always returns a nonzero result handle when the raw input range is valid,
including for ordinary language/runtime errors. Operation IDs and request payloads
use `meco-wire/1`: fixed little-endian integers and length-prefixed strict UTF-8
byte strings, with unknown required fields rejected. Each operation documents its
exact payload before implementation; no Rust layout, pointer, enum discriminant,
JSON number, or JavaScript object layout crosses the ABI.

Result access is read-only and bounded:

```text
u32 meco_result_status(u32 result)             // success or structured error
u32 meco_result_value_handle(u32 result)       // 0 when absent
u32 meco_result_payload_length(u32 result)
u32 meco_result_payload_copy(u32 result, u32 destination, u32 capacity)
void meco_handle_dispose(u32 handle)
```

Payload copy returns the required length without writing when capacity is too
small. A value handle has an internal kind (package builder, compiled grammar,
session, repetition store, result, snapshot, or replay bundle). Every operation
checks kind and liveness.

Handle `0` is invalid. Public handles are monotonically allocated `u32` values and
are never reused during one instance lifetime; reaching the ID limit returns
`E_ABI_HANDLE_EXHAUSTED`. Disposal removes the value. Stale, double-disposed,
unknown, and cross-kind handles return structured `E_ABI_*` diagnostics. Dropping
the WebAssembly instance releases all remaining handles and allocator state.

### JavaScript and Deno errors

The TypeScript wrapper turns every result payload into:

```ts
type MecoResult<T> =
  | { ok: true; value: T; diagnostics: Diagnostic[] }
  | { ok: false; error: MecoError; diagnostics: Diagnostic[] };
```

It uses fatal UTF-8 encode/decode behavior, deterministic `bigint` conversion for
64-bit fields, and `try/finally` disposal. An optional `orThrow()` throws a wrapper
`MecoError`; ordinary core failures never surface as an unclassified JS exception.
The wrapper rejects unpaired UTF-16 surrogates before allocation. Deno is the
normative JS integration host; browser tests load the identical `.wasm` artifact.

## CLI streams and statuses (`cli/1`)

The optional `std` CLI implements this fixed matrix:

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
stdout only when explicitly requested. `check`, `lint`, `generate`, `audit`,
`manifest`, `migrate`, and `bench` follow this same contract. Subprocess fixtures
in Milestone 9 must prove every row before `cli/1` is stable.
