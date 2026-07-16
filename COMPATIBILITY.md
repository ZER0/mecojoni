# Mecojoni v2 compatibility policy

These identifiers are compatibility contracts, not marketing labels.

| Surface | Current contract | Compatible evolution |
| --- | --- | --- |
| Source language | `meco: 2` | Clarifications and new optional front-matter fields only when old valid source keeps its meaning; incompatible syntax requires `meco: 3`. |
| Rust semantic API | `API_VERSION = 2` | Additive APIs before crate publication; removals or semantic changes require a major crate version and documented migration. |
| Rational arithmetic | `rational/1` | Exact vectors cannot change under the same identifier. |
| PRNG | `splitmix64/1` | Seed-to-word vectors cannot change. |
| Independent sampler | `weighted/1` | Eligible weights, normalization, PRNG consumption, and seed mapping cannot change. Internal indexes may change only when outputs/traces remain identical. |
| Diverse sampler | `diverse/1`, `location/1` | Candidate reservation, scoring, cooldown, histories, tie-breaking, and commit semantics are frozen together. Retuning requires a new slash-version. |
| Composition audit | `composition/1` | Thresholds/tokenization require a new profile identifier. |
| Text normalization | `ascii-fold-whitespace/1` | Exact normalized keys cannot change. |
| Fragment tokenizer | `scalar-word/1` | Fragment boundaries cannot change. |
| Production IDs | `production-fnv1a64/1` | Authored IDs remain verbatim; derived IDs may change only under a new hash contract and explicit state migration. |
| WASM ABI/wire | ABI `1`, wire `1` | New exports and operation numbers are additive. Existing signatures, handle ownership, payload fields, and operation meanings cannot change. Unknown trailing input remains an error. |
| Snapshots | `snapshot/1`, `MECS` / `MECR` | Existing bytes decode identically. New incompatible state requires a new version and fails closed in old readers. |
| CLI | `cli/1` | Commands may gain optional flags; stream placement, JSON field meaning, flag parsing, and exit statuses remain compatible. |
| Benchmarks | `workloads/1` | Exact operation fixtures change only with a reviewed compatibility decision. |

Diagnostic codes are stable machine identifiers. A code's category and meaning
must not be reused; new failure cases get new codes. Human messages may improve,
and exact spans may become narrower, but severity cannot silently weaken. Hosts
must branch on codes rather than English text.

Replay receipts record the grammar artifact hash plus sampler, normalizer,
tokenizer, snapshot, and state revisions needed to reject incompatible replay.
Migration never claims v1/v2 seed compatibility.

The crates remain unpublished (`publish = false`) until the project owner chooses
distribution versioning and a root license. That administrative choice does not
weaken the frozen on-disk/runtime contracts above.
