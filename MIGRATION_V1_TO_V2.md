# Migrating Mecojoni v1 to v2

The v2 CLI contains a frozen, dependency-free reader for the actual v1 lexical
rules. It does not parse v1 as if it were v2.

```sh
cargo +1.85.0 run -p mecojoni-cli -- \
  migrate old-dialogue.meco --write new-dialogue.meco.md
cargo +1.85.0 run -p mecojoni-cli -- check new-dialogue.meco.md
```

The rewrite creates strict v2 front matter, retains the v1 `@start` rule as
`entry` and the sole initial export, chooses `diverse/1` to preserve the purpose
of v1's default varied sampler, preserves decimal weight spellings, and uses
braced references. Every v1 terminal segment is quoted as data. Consequently:

- `@@`, `$`, and `&` cannot accidentally become v2 syntax;
- `@empty` and `ε` become the explicit `""` body;
- `//` comments become safe `<!-- -->` comments;
- references adjacent to identifier-like text remain unambiguous;
- whitespace discarded by the v1 reader is not reintroduced as visible output.

Migration diagnostics identify ambiguous/discarded whitespace, changed sigils,
empty output, comment rewrites, and leading bracket prose that resembles a
weight. `--deny-warnings` lets CI require manual review.

Source migration cannot preserve runtime sequences. The command always reports
these behavioral differences:

- v1 string seeds use `mulberry32`; v2 `u64` seeds use `splitmix64/1`;
- `diverse/1` preserves v1 varied-selection intent, not its exact candidate
  scoring and history sequence;
- content-derived stable production IDs are new unless explicit IDs are added.

The checked-in subprocess corpus migrates a real weighted/reference/empty/sigil
v1 file, compiles it as v2, and confirms generated text stays within the original
finite output set. It deliberately does not claim equal seed-to-output mapping.
