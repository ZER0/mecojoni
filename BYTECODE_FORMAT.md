# Mecojoni `bytecode/1` format

`bytecode/1` is the frozen, little-endian compiled container produced by the v2
toolchain. Source remains authoritative; `.mecob` is a reproducible deployment
artifact. The decoder treats every byte as untrusted and constructs no partial
grammar.

## Compatibility

- Magic is `MECB`; container major/minor is `1.0`.
- The source language is exactly `meco: 2`, core API is `2`, and the current
  lowered contract is `lowered-ir/1`.
- The 16-byte runtime fingerprint is `meco-bc1-0000001`. A runtime accepts only
  that fingerprint. Compatible implementations preserve it; an incompatible
  lowered contract requires a new fingerprint and bytecode major.
- All indexes and counts are `u32`; byte lengths and offsets are `u64`.
- Numbers are reduced `(i64 numerator, u64 denominator)` values under
  `rational/1`. There is no floating-point representation.
- The bytecode hash is dependency-free FNV-1a64 with header bytes 48–55 treated
  as zero. It detects accidental corruption, not malicious replacement.
- Authenticity is owned by the host distribution/signing layer.

## Container

The fixed 72-byte header is:

| Offset | Width | Field |
| ---: | ---: | --- |
| 0 | 4 | `MECB` |
| 4 | 2 | major `1` |
| 6 | 2 | minor `0` |
| 8 | 4 | header bytes `72` |
| 12 | 4 | profile: full `0`, mapped `1`, stripped `2` |
| 16 | 8 | exact total bytes |
| 24 | 4 | section count `1` |
| 28 | 4 | source version `2` |
| 32 | 4 | core API version `2` |
| 36 | 4 | reserved zero |
| 40 | 8 | semantic package hash |
| 48 | 8 | bytecode content hash |
| 56 | 16 | runtime fingerprint |

One 32-byte required-section record follows: kind `1`, required flag `1`, record
width `0`, offset `104`, exact remaining length, count `1`, reserved zero. No
other sections, gaps, or trailing bytes are valid in bytecode/1.

All three profile values retain the same lowered runtime spans and warnings;
source text and source names are absent from every profile. In bytecode/1 the
profile is a tooling-capability declaration, not a compression switch. `mapped`
and `stripped` reject full-debug-only requests with `E_BYTECODE_CAPABILITY` while
preserving exact generation and provenance semantics.

## Lowered grammar payload

Records are concatenated without padding. A vector is `u32 length` followed by
its records. A string is `u32 UTF-8 byte length` followed by exact bytes. A bool
is exactly `u8 0` or `1`. An optional value is a bool followed by the value when
present. A span is source `u32` plus start-byte, start-scalar, end-byte, and
end-scalar `u64` values.

The grammar record contains, in order:

1. inputs: name plus value type;
2. rules;
3. public entries: qualified name plus rule index;
4. default rule index, with `u32::MAX` meaning none;
5. compiler warnings: code, warning severity `1`, optional span, message; and
6. complete-message definitions and ordered argument schemas.

A rule stores name, typed parameters, span, four analysis bits (reachable,
productive, nullable, recursive), message-effect bool, optional static cumulative
selection, and productions. A production stores stable ID, authored-ID bool,
span, weight, optional guard, ordered bindings, body parts, and the 16.16
diversity factor.

Frozen tag assignments are:

| Record | Tags |
| --- | --- |
| Value type/schema | text `0`, number `1`, boolean `2`, enum `3` |
| Constant | text `0`, number `1`, boolean `2`, enum member `3` |
| Value operand | input `0`, local `1`, constant `2` |
| Weight | static rational `0`, dynamic expression `1` |
| Weight expression | rational `0`, value `1`, add `2`, subtract `3`, multiply `4` |
| Guard | value `0`, is `1`, is-not `2`, less `3`, less-or-equal `4`, greater `5`, greater-or-equal `6`, not `7`, and `8`, or `9` |
| Guard operand | value `0`, constant `1` |
| Body part | literal `0`, rule call `1`, emitted value `2`, emitting capture `3`, complete message `4` |

Binary expressions use prefix order. Calls store a target rule index followed by
ordered compiled values. Bindings additionally store their assigned local slot,
name, and span. Message calls store the stable message ID and ordered named
values. All cached selections and analysis facts are verified before generation.

## Limits and rejection

The hard artifact limit is 64 MiB and decoded logical limit is 128 MiB. Default
independent ceilings are 1,000,000 strings, 100,000 rules, 1,000,000
productions, 4,000,000 instructions/operands, recursion depth 256, and 100,000
diagnostics. Callers may lower but not raise hard ceilings.

Stable failures are `E_BYTECODE_MAGIC`, `E_BYTECODE_VERSION`,
`E_BYTECODE_CORRUPT`, `E_BYTECODE_LIMIT`, `E_BYTECODE_CONTRACT`, and
`E_BYTECODE_CAPABILITY`. The decoder validates header/directory arithmetic and
hash before allocation-heavy payload decoding, then validates UTF-8, tags,
rationals, spans, indexes, local-slot order, arity, message schemas, cached
selection monotonicity, and the shared lowered-grammar invariants.

## Canonical output

Source compilation assigns source ID `0` to the root module, then orders all
remaining modules by canonical ID and assigns consecutive source IDs. Encoding
preserves semantic argument order, exact stable IDs, canonical spans, warnings,
entries, and manifests.
Repeated encoding and decode/re-encode must be byte-identical; changing only the
profile changes the profile field and content hash, not semantic package hash.
Timestamps, source names and paths, allocator state, and random metadata are
forbidden.
