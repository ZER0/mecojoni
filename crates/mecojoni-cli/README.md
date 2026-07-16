# Mecojoni authoring CLI

`meco` is the optional `std` author/build tool for the dependency-free
`mecojoni-core`. Filesystem traversal, process exit statuses, clocks used by
benchmarks, and terminal streams live here and never enter the portable core.

Frozen `bytecode/1` commands compile complete filesystem packages atomically,
inspect and verify hostile `.mecob` input, and generate through the same runtime:

```sh
meco compile-artifact root.meco --messages messages.manifest --profile full --write root.mecob
meco inspect-artifact root.mecob
meco verify-artifact root.mecob
meco generate-artifact root.mecob --seed 7
```

`--output` remains the established text/JSONL stream selector, so the artifact
destination uses `--write`.

Build or run it from the workspace root:

```sh
cargo +1.85.0 build -p mecojoni-cli
cargo +1.85.0 run -p mecojoni-cli -- check path/to/root.meco
```

The command surface is versioned as `cli/1`:

| Command | Purpose |
| --- | --- |
| `check` | Recursively load imports, parse, compile, and validate a v2 package. |
| `generate` | Generate one or more independent `weighted/1` results. |
| `trace` | Generate with stable production-selection and work traces. |
| `lint` | Report compiler warnings and reachable `composition/1` findings. |
| `audit` | Sample traced output and report structural/rendered repetition. |
| `manifest` | Export the compiled host-input and external-message schema. |
| `migrate` | Read frozen v1 semantics and emit explicit v2 source. |
| `fmt` | Validate and apply the conservative `format/1` source contract. |
| `bench` | Measure elapsed time and deterministic expansion/sampler work. |

Common options accept both `--flag value` and `--flag=value`. Use
`--output jsonl` for one versioned object per result/report, `--entry` for an
explicit qualified export, `--seed` for a `u64`, and repeat
`--data name=value` for typed package inputs. `--deny-warnings` changes a
completed warning-bearing report to status 1. Duplicate scalar flags, unknown
flags, and a flag consumed as another flag's missing value are usage errors.
Message-bearing packages pass `--messages path/to/messages.manifest`; its
dependency-free line format is `message-id|argument:type,...`. This supplies the
same explicit schema required by the core and lets `check` and `manifest` validate
and export localized-message boundaries without reading catalogs.

Text generation writes each complete text followed by one LF. It buffers the
requested batch first, so a later generation failure cannot leave partial
success records. Text traces go to stderr; JSONL traces stay inside their result
object. Exit statuses are 0 success, 1 language/data/generation or requested
warning failure, 2 usage/host I/O, and 3 unexpected internal failure.

## Formatter contract

The first `format/1` contract is intentionally conservative: it validates the
complete v2 module and returns it byte for byte. This is a semantic safety
boundary for editor/build integration—comments, literal edge spaces, and block
chomp behavior cannot drift—while style-changing rewrites remain unspecified.
Tests compile and generate checked-in files before/after formatting.

## Tests

```sh
cargo +1.85.0 test -p mecojoni-cli --all-targets
```

The integration suite invokes the built `meco` subprocess for every command,
loads real imported v2 packages and v1 files from disk, and covers text/JSONL,
stdout/stderr separation, warning thresholds, no-partial-output behavior, both
flag spellings, and all four exit statuses.
