<p align="center">
  <img src="v1/assets/mecojoni-logo.png" alt="Mecojoni" width="760">
</p>

<p align="center">
  A Markdown-like language for compositional, localized generative text.
</p>

# Mecojoni v2

Mecojoni v2 is a readable, typed, modular language for generative dialogue and
text. It retains the useful core of a weighted context-free grammar—headings
define rules and list items define alternatives—
while adding the structure needed for game data, conditions, reuse, localization,
and reliable long-running generation.

> **Status:** the v2 source-language and compiled-artifact roadmaps are
> feature-complete. Representative native/WASM measurements justified freezing
> the hostile-input-checked `bytecode/1` format and its single-WASM deployment
> path. The
> dependency-free core parses and compiles complete packages, executes exact typed
> `weighted/1` generation, and resolves complete localized messages through a
> synchronous host formatter in Rust, Deno, and Chrome. Transactional `diverse/1`, span-aware
> provenance, repetition audits, replay receipts, and versioned session/history
> snapshots now run across Rust and WASM. The dependency-free `std` authoring CLI,
> explicit v1 migration, initial editor grammar, committed v1/native/WASM workloads,
> and frozen compatibility contracts are also executable. The crates remain
> unpublished pending the owner's distribution version and license choice. The
> original proof of concept and its documentation live in [`v1/`](v1/README.md).
> The bytecode design and implementation status are tracked in
> [`BYTECODE_FORMAT_PLAN.md`](BYTECODE_FORMAT_PLAN.md); recorded evidence lives in
> [`BENCHMARKS.md`](BENCHMARKS.md).

The current Rust API can canonically `encode_artifact`, `decode_artifact`,
`inspect_artifact`, and `disassemble_artifact`. The hostile-input-checked format
is frozen as `bytecode/1`; see [`BYTECODE_FORMAT.md`](BYTECODE_FORMAT.md) for its
exact layout and [`COMPATIBILITY.md`](COMPATIBILITY.md) for evolution rules.

The authoring CLI can compile, inspect, verify, and generate `.mecob` files.
Deno and browser hosts can inspect or load the same bytes through
`Mecojoni.inspectArtifact` and `Mecojoni.loadArtifact`; loaded artifacts use the
ordinary grammar-handle lifecycle and leak telemetry.

`deno task wasm:embedded:build` produces a content-specific WASM whose private
data segment contains the fully resolved Harbor artifact. `openEmbeddedGrammar()`
opens it without a content fetch; the generic WASM returns a capability error.
The Chrome smoke test asserts that the content build requests neither `.meco`
nor `.mecob` at runtime.

The syntax in this README is authoritative. `V2_SPECIFICATION.md` must be updated
with every syntax change; if the documents temporarily disagree, this README wins.

## Why v2?

The v1 format proves that Markdown headings, list items, references, weights, and
seeded generation make an approachable grammar format. It deliberately leaves out
host data, imports, rule parameters, conditions, localization, and a formal
whitespace model. Those omissions keep a small prototype small, but they become
limiting as a grammar becomes a game-facing content system.

V2 keeps authored text at the centre. The visible output remains easy to scan:

```meco
# greeting
- Hello, @person!
- @person, @observation.
```

The additional syntax is reserved for structure that does not itself emit text:

```meco
- {mood is tense}
  {common.name as hero}
  &arrival <- hero: $hero
```

The braces establish eligibility and data; the final line is the complete
localized message that will be rendered.

## V1 and v2 at a glance

| Area | V1 | V2 design |
| --- | --- | --- |
| Source layout | Directives plus Markdown rules | Strict front matter plus Markdown rules |
| Default target | Required `@start` | Optional root `entry`; otherwise the host chooses an export |
| References | `@rule` | `@rule`, with `@{rule}` for explicit boundaries and captures |
| Empty output | `@empty` or `ε` | `""` |
| Data-dependent weights | Not available | `[weight = expression]` over immutable numeric inputs and parameters |
| Literal `@` | `@@` | `\@`, quoted strings, or raw literals |
| Comments | Whole-line `//` | Markdown comments: `<!-- ... -->` |
| Inputs and types | Not available | Typed front-matter inputs and finite types |
| Rule parameters | Not available | `# rule <- value: type` and `@rule <- value: $input` |
| Conditions | Not available | Non-emitting guards such as `{mood is tense}` |
| Reuse one generated value | Not available | Emitting captures and non-emitting bindings |
| Imports and visibility | Not available | Modules, aliases, imports, and explicit exports |
| Localization | Not available | Complete external messages through `&message` |
| Sampler configuration | Runtime `random` / `varied` option | Optional `weighted/1` or `diverse/1` authoring default, overridable by the host |
| Safety and analysis | Reachability/productivity checks | Typed calls, effects, module visibility, iterative limits, traces, audits, and replay-oriented sessions |

## Quick start

V2 source begins with a small, strict front-matter header. This sample has no
default entry, so a host must request one of its exported rules explicitly.

```meco
---
meco: 2
module: npc
sampler: diverse/1

types:
  Mood: [calm, tense]

inputs:
  playerName: text
  itemCount: number
  mood: Mood

imports:
  common: "./common.meco"

exports: [pickup, greeting, warning]
---

# pickup
- [3] &pickup-common <- player: $playerName, count: $itemCount
- [1] {mood is tense}
  &pickup-alert <-
    player: $playerName
    count: $itemCount

# local-intro
- @{common.name as hero} arrived. $hero looked tired.
```

`pickup` chooses a complete localized message. The second production is eligible
only when the `mood` input is `tense`. The argument values are data, not visible
text; `&pickup-common` or `&pickup-alert` owns the complete rendered result.

Add `entry: pickup` to a package root when it should have a default generation
target. V2 never guesses a default from the first rule or the order of `exports`.

## Complete v2 example corpus

The following source is the canonical v2 syntax corpus. It intentionally places
many independent examples in one file so each form can be reviewed in context.

```meco
---
meco: 2
module: npc
sampler: diverse/1

types:
  Mood: [calm, tense]

inputs:
  playerName: text
  itemCount: number
  mood: Mood

imports:
  common: "./common.meco"

exports: [pickup, greeting, warning]
---

<!-- No entry is declared: a caller must select an exported rule. -->

<!-- Arguments after <- supply data; they are not visible output themselves. -->
# pickup
- [3] &pickup-common <- player: $playerName, count: $itemCount
- [1] {mood is tense}
  &pickup-alert <-
    player: $playerName
    count: $itemCount

<!-- An emitting capture selects once, emits once, then makes $hero reusable. -->
# local-intro
- @{common.name as hero} arrived. $hero looked tired.

<!-- Braced clauses are silent. Guards come before bindings. -->
# localized-arrival
- {common.name as hero}
  &arrival <- hero: $hero

# localized-encounter
- {common.name as hero}
  {common.name as companion}
  {common.place as destination}
  &encounter <- $hero, $companion, $destination

# title-suffix
- [3] ""
- [1] " the "@common.title

# multiline-example
- |
  First line.
  Second line.

# tense-arrival
- [1] {mood is tense}
  {common.name as hero}
  &arrival <- $hero

# tense-arrival-with-companion
- [1] {mood is tense}
  {common.name as hero}
  {common.name as companion}
  &arrival <- hero: $hero, $companion

<!-- Basic composition and public rules. -->
# greeting
- [3] @salutation, @person!
- [1] @person, @observation.

<!-- The header declares a parameter; <- at the call site supplies it. -->
# greetings <- name: text
- Hello, $name!
- Welcome back, $name!

# player-greeting
- @greetings <- name: $playerName

# warning
- Attention, @person: @observation.

# salutation
- Hello
- Good morning
- Welcome

# person
- traveller
- neighbour
- old friend

# observation
- the weather has changed
- the market is unusually quiet
- today feels promising

<!-- A minimal subject-predicate grammar. -->
# sentence
- @subject @predicate.

# subject
- The pilot
- A maintenance drone

# predicate
- is waiting
- found the missing tool

<!-- Ordinary unweighted alternatives. -->
# temperature
- cold
- mild
- uncomfortably warm

<!-- References embedded in terminal text. -->
# report
- The @device is @condition.

# device
- air recycler
- navigation console

# condition
- offline
- making a strange noise

<!-- Integer and decimal relative weights. An omitted weight is 1. -->
# weighted-mood
- [6] calm
- [3] tired
- [1] furious
- [0.5] cautiously optimistic

<!-- Empty output, optional text, and an explicitly delimited adjacent reference. -->
# titled-greeting
- Welcome, @{name}@title-option.

# title-option
- [3] ""
- [1] " the "@title

# name
- Ada
- Tomas

# title
- Captain
- Doctor

<!-- A delimited reference separates the rule name from a literal suffix. -->
# creature-count
- Several @{creature}s arrived.

# creature
- traveller
- maintenance drone

<!-- Productive recursion with a strongly preferred terminating production. -->
# inventory
- [5] @item
- [1] @item, @inventory

# item
- a coil of wire
- a repair kit
- an empty canister

# calm-line
- Everything is under control.

<!-- Escape a sigil when it should be emitted literally. -->
# contact
- Send a message to pilot\@example.invalid.

<!-- A raw string does not interpret sigils or escape sequences. -->
# raw-contact
- r"Send a message to pilot@example.invalid."

<!-- A raw block keeps every sigil as literal text and strips its final newline. -->
# raw-sigils
- |raw-
  @person, $playerName, and &pickup-alert are literal text.

<!-- Split incompatible concepts into semantically coherent branches. -->
# incident
- @repair-incident
- @travel-incident

# repair-incident
- @technician @repair-action @broken-device.

# travel-incident
- @traveller @travel-action @destination.

# technician
- The mechanic
- A service drone

# repair-action
- inspected
- repaired

# broken-device
- the air recycler
- the navigation console

# traveller
- The courier
- A survey team

# travel-action
- departed for
- finally reached

# destination
- the northern outpost
- the orbital terminal
```

## The `.meco` format

`.meco` is the canonical v2 source extension used by repository fixtures,
imports, examples, editor integration, and generated documentation. The portable
parser still consumes host-supplied source independently of its filename.

### Front matter, modules, and entries

Every module declares `meco: 2` and a `module` name. The header is a strict
Mecojoni schema, not general-purpose YAML: unknown or duplicate fields are
errors, as are YAML tags, anchors, aliases, merges, and implicit values.

The root may additionally declare:

| Field | Purpose |
| --- | --- |
| `entry` | Optional default public rule for generation requests that do not name one. |
| `sampler` | Optional authoring recommendation such as `diverse/1`. |
| `types` | Named finite types, for example `Mood: [calm, tense]`. |
| `inputs` | Typed data supplied by the host. |
| `imports` | Module paths mapped to aliases. |
| `exports` | The rules that callers may select. |

Rules are private unless exported. Imported references use their alias:

```meco
imports:
  common: "./common.meco"

# line
- @common.name arrived.
```

### Rules, references, and visible text

A `# heading` defines a rule and each `-` item is a weighted alternative. Rule
references expand inline and emit their result:

Initial v2 identifiers are case-sensitive ASCII; terminal text may contain any
valid UTF-8. Unicode identifiers are deferred so the portable core does not carry
normalization tables before a real authoring requirement justifies them.

```meco
# report
- The @device is @condition.
```

Use `@{name}` when a suffix would otherwise make the reference ambiguous:

```meco
- Several @{creature}s arrived.
```

Use an emitting capture when the selected output must be reused later in the same
production:

```meco
- @{common.name as hero} arrived. $hero looked tired.
```

The capture selects `common.name` once, emits it once, and binds that same value
as `$hero`. Captures are local to their production and candidate; nested rules
receive values only through declared parameters.

### Silent clauses: guards and bindings

Leading braces describe work that does not itself produce visible text:

```meco
- {mood is tense}
  {common.name as hero}
  &arrival <- $hero
```

`{mood is tense}` is a guard. It determines whether the production is eligible.
`{common.name as hero}` is a binding. It expands a rule once and stores the value
without emitting it. Guards must come before bindings: eligibility is decided
before the runtime selects a production and evaluates its bindings.

A later binding may call a parameterized rule with values from earlier bindings:

```meco
- {common.name as hero}
  {common.companion <- owner: $hero as companion}
  $hero arrived with $companion.
```

The `as companion` suffix names the silent result; the arguments between `<-`
and `as` follow the same named and punned rules as an emitting call.

The first non-braced item is the visible body. This is why a normal textual body
does not need a separator:

```meco
- {mood is tense}
  {common.name as hero}
  $hero, I see you are with your companion.
```

### Typed rule parameters and calls

Rule headers declare typed parameters with `<-`:

```meco
# greetings <- name: text
- Hello, $name!

# player-greeting
- @greetings <- name: $playerName
```

The value after `<-` is an argument list, not output. The call expands
`greetings`, passing the host input as its `name` parameter, then emits the chosen
rule result. The same syntax calls a localized message:

```meco
&arrival <- hero: $hero, companion: $companion
```

Within an argument list, `$hero` is shorthand for `hero: $hero`. Therefore:

```meco
&encounter <- $hero, $companion, $destination
```

means:

```meco
&encounter <- hero: $hero, companion: $companion, destination: $destination
```

`<-` is only a call-argument operator after `@rule` or `&message`; it is never a
general assignment form.

### Complete localized messages

`&message` resolves a stable external message through the configured formatter.
It must be the complete visible body of a production, rather than a fragment
inside English prose. This lets each locale control word order, agreement,
plurality, and inflection.

```meco
- {common.name as hero}
  &arrival <- $hero
```

The binding contributes data but no text. The formatter owns the rendered result.
A message-valued rule cannot be captured, suffixed, or wrapped in another visible
rule fragment.

Compilation receives a formatter manifest, so missing IDs, missing or extra
arguments, and type drift fail before generation. A generated message crosses the
host boundary as one ordered request containing its stable ID, typed values,
requested locale, and explicit fallback chain. The formatter returns the complete
text plus actual locale, environment identity, diagnostics, deterministic work
units, and whether the result is replayable. It is synchronous and operates only
over resources the host has already loaded; the core performs no locale I/O and
contains no built-in plural engine.

The Rust API exposes `compile_package_with_manifest`,
`generate_weighted_structural`, and `generate_weighted_with_formatter`. The
browser-neutral TypeScript wrapper accepts the same contract:

```ts
const compiled = meco.compilePackage(packageDescription, {
  messages: [{
    id: "arrival",
    arguments: [
      { name: "hero", type: { kind: "text" } },
      { name: "count", type: { kind: "number" } },
    ],
  }],
});
if (!compiled.ok) throw new Error(compiled.error.message);

const result = meco.generateWeighted(compiled.value, {
  seed: 7n,
  locale: "pl",
  fallbackLocales: ["en"],
  data: { itemCount: { kind: "number", numerator: 2n, denominator: 1n } },
  formatter: (request) => formatFromPreloadedCatalog(request),
});
```

See [`js/README.md`](js/README.md) and
[`V2_INTERFACES.md`](V2_INTERFACES.md) for the concrete callback and wire
contracts.

### Weights, empty output, and recursion

Weights are positive relative base weights. Omitted weights are `1`, and decimals
are valid:

```meco
# mood
- [6] calm
- [3] tired
- [1] furious
- [0.5] cautiously optimistic
```

A dynamic weight may use an immutable numeric input or rule parameter. It is
evaluated before selection; a value of zero makes that production ineligible.

```meco
# reaction <- urgency: number
- [weight = urgency] The alarm is spreading.
- [1] Everything is quiet.
```

Dynamic weight expressions use bare names, as guards do. They may use decimal
literals, number inputs/parameters, parentheses, `+`, `-`, and `*`; they cannot use
captures, generated rules, messages, callbacks, clocks, or ambient state. This
keeps the result deterministic and replayable.

Version `rational/1` evaluates those values exactly as reduced signed fractions:
the absolute numerator and positive denominator are each at most `2^63 - 1`.
Decimal literals contain at most 18 digits and an optional exponent from `-18` to
`18`; an operation or per-rule scaled total outside the budget is an error rather
than a floating-point approximation. Version `splitmix64/1` supplies the seeded
random stream, whose fixed vectors are shared by Rust and the Deno/WASM wrapper.

An entire production containing `""` emits nothing:

```meco
# title-suffix
- [3] ""
- [1] " the "@title
```

Recursive rules are valid only when there is a productive route back to terminal
text. Terminating alternatives should normally carry most of the weight:

```meco
# inventory
- [5] @item
- [1] @item, @inventory
```

### Whitespace, strings, blocks, comments, and escapes

Visible text preserves its authored characters. Put intentional leading or
trailing whitespace inside a quoted segment:

```meco
- " the "@title
```

Double-quoted segments interpret escapes. `r"..."` is a raw single-line literal;
`|raw`, `|raw-`, and `|raw+` are raw block forms. `|` retains one final newline,
`|-` removes it, and `|+` preserves trailing blank lines.

```meco
- Send a message to pilot\@example.invalid.
- r"Send a message to pilot@example.invalid."
- |raw-
  @person and $playerName are literal text here.
```

Outside literals, Markdown comments are ignored:

```meco
<!-- This production is for calm conditions. -->
# calm-line
- Everything is under control.
```

## Sampling and reproducibility

V1 exposes two runtime modes: independent `random` and repetition-resistant
`varied`. V2 makes the policy names and versions explicit:

| V2 policy | Corresponding v1 behavior | Use |
| --- | --- | --- |
| `weighted/1` | `random` | Exact independent weighted CFG draws. |
| `diverse/1` | `varied` | Stateful repetition resistance for player-facing text. |

Under `weighted/1`, weights are exact relative probabilities. Under `diverse/1`,
they remain authorial priors but may be adjusted by bounded structural cooldown,
subtree diversity, candidate search, and surface-novelty scoring. Nullable and
recursion-sensitive rules retain their termination and optionality weights.

`sampler: diverse/1` is a recommendation stored with the grammar, not an
unchangeable semantic property. A host may explicitly override it. The effective
sampler version, settings, grammar hash, seed, input, locale, and requested entry
must be recorded for reproducible sessions.

The initial `diverse/1` profile, `location/1`, uses 12 candidate attempts, an
immediate-reuse gap of one selection, a four-selection soft cooldown horizon,
3–8 word edge fragments, 300 retained edge records, and 50,000 retained exact
records. The edge and exact histories also have 4 MiB and 16 MiB canonical UTF-8
logical-byte caps. These are versioned profile values, not hidden tuning constants.
The default resource profile preserves v1's depth limit of 80 and expansion limit
of 2,000 per candidate while also bounding output, sampling work, and rendered bytes.
The complete profile and limit tables are normative in
[V2_SPECIFICATION.md](V2_SPECIFICATION.md).

The executable API keeps mutable state explicit. `SamplerSession` owns the parent
PRNG and call order; `RepetitionStore` owns structural, exact-output, and edge
histories and may be shared deliberately. `DiverseGenerationRequest` contains no
seed because the session is the sole random source. Each successful `location/1`
call reserves 12 substream seeds, commits one winner and one store revision, and
reports its attempt and novelty score. Any failure leaves both objects unchanged.

```rust
let mut session = SamplerSession::new(42);
let mut repetition = RepetitionStore::new_location();
let result = session.generate(
    &grammar,
    &mut repetition,
    &DiverseGenerationRequest::default(),
)?;
```

Successful diverse results include a `ReplayReceipt` containing grammar and state
hashes, the request digest, fixed PRNG reservation, stable derivation hash, winner,
final-text hash, and post-commit revision. `SamplerSession::snapshot()` and
`RepetitionStore::snapshot()` capture the pre-call continuation; restoring both
after nonempty history reproduces the next result. In-memory repetition snapshots
share an immutable copy-on-write root. Serialized snapshots use `snapshot/1`, are
bounded to 64 MiB at the decoder, and validate their profile windows and logical
byte declarations before allocating live state.

Repetition snapshots contain resolved lines and fragments and must therefore be
treated as sensitive data. `SnapshotPolicy` makes capture consent, logical budget,
pinning, and revision-relative expiry explicit. A pin is ordinary caller ownership:
dropping the snapshot releases it, so the core never grows an ambient retention
registry. Durable storage and encryption remain host responsibilities.

Production identities are not list positions. An explicit `id` remains stable
across weight, order, and prose edits; otherwise the compiler derives a content-
addressed artifact-local ID from the qualified rule and canonical body, excluding
weight. Duplicate unlabeled alternatives and all within-rule ID collisions are
compile errors.

## Compilation, generation, and diagnostics

The proposed compiler validates source before any generation. Its checks include:

- front-matter shape, module identity, imports, exports, and visibility;
- duplicate or undefined rules, invalid references, parameters, and call arguments;
- input and parameter types, guard expressions, duplicate bindings, shadowing,
  forward references, and unused bindings;
- message-effect placement: a localized message must own the complete visible body;
- productive reachable rules, nullable paths, recursive components, and risk;
- weights, strings, escapes, comments, blocks, and source spans.

Generation uses an explicit expansion stack and configured limits rather than the
host language call stack. Failed, losing, cancelled, or over-budget diverse
candidates roll back their bindings and sampler state. Successful generations can
return a trace that identifies selected rules, production identities, binding
events, source locations, sampler adjustments, and formatter/message work.
With `trace_provenance`, every visible byte/scalar range is linked to authored
text, host or bound values, emitting captures, or a coarse complete message.
Non-emitting bindings retain derivation links but no output range. Structural and
rendered repetition audits operate on these retained traces; rendered findings
attribute a fragment only to nodes whose output ranges overlap it.

`composition/1` is an optional, deliberately strict audit heuristic. It warns
when a sentence-ending locally composed production has fewer than three direct
emitting grammar references or an authored literal run longer than two words.
Complete `&message` bodies are exempt because their structure belongs to the
formatter. It is a signal for review, never a prose-quality verdict.

## Authoring guidance

The number of possible outputs is less important than whether each combination
makes sense. Organize a grammar around semantic contracts:

```meco
# incident
- @repair-incident
- @travel-incident

# repair-incident
- @technician @repair-action @broken-device.

# travel-incident
- @traveller @travel-action @destination.
```

This prevents repair actions from being combined with travel destinations merely
because both were placed in global pools. Use weights for intentional world
texture, keep recursion termination-biased, and use localized messages when the
sentence must adapt to locale-specific grammar.

## Migration from v1

The `meco migrate` command parses v1 with frozen v1 semantics, then emits explicit
v2 source. It never silently reinterprets old files:

```sh
cargo +1.85.0 run -p mecojoni-cli -- \
  migrate dialogue.meco --write dialogue.meco
cargo +1.85.0 run -p mecojoni-cli -- check dialogue.meco
```

| V1 source | V2 migration |
| --- | --- |
| `@meco 1` | `---` front matter with `meco: 2` and `module` |
| `@start greeting` | Optional root `entry: greeting` |
| `@name` | `@name`, or `@{name}` where a suffix boundary is needed |
| `@empty` / `ε` as a whole production | `""` |
| `@@` | `\@` or an appropriate raw/quoted literal |
| `// comment` | `<!-- comment -->` |
| Runtime `random` / `varied` | Optional `weighted/1` / `diverse/1` default or host configuration |

Migration must diagnose constructs that cannot be preserved safely without a
quoted or raw rewrite, especially significant whitespace and newly meaningful
sigils.

The tool reports that v1/v2 PRNG streams and diversity scoring are intentionally
not sequence-compatible. See [MIGRATION_V1_TO_V2.md](MIGRATION_V1_TO_V2.md) for
the exact rewrite and compatibility contract.

## Authoring CLI and editor grammar

The optional `mecojoni-cli` crate provides `check`, `generate`, `trace`, `lint`,
`audit`, `manifest`, `migrate`, `fmt`, and `bench`. It recursively resolves imports
from an explicit root while the portable core continues to perform no I/O.

```sh
cargo +1.85.0 run -p mecojoni-cli -- check npc.meco
cargo +1.85.0 run -p mecojoni-cli -- \
  generate npc.meco --seed 7 --data playerName=Rin
cargo +1.85.0 run -p mecojoni-cli -- lint npc.meco --deny-warnings
```

Human output and `cli/1` JSONL have fixed stdout/stderr and exit-status contracts.
Generation batches are buffered before output, so errors never leave partial
success records. The initial formatter validates then preserves source byte for
byte; this proves comments, edge spaces, and block chomp semantics cannot change
while style-changing rules remain deliberately unspecified. Full CLI details are
in [the CLI guide](crates/mecojoni-cli/README.md).

[`editors/vscode/`](editors/vscode/README.md) supplies an initial TextMate grammar
and language configuration. Semantic diagnostics use `meco check`; an LSP
transport is deferred until real editor synchronization requirements justify it.

## Tooling and implementation

The primary implementation is Rust. Its core is `#![no_std]` plus `alloc`,
with no filesystem, network, clock, thread-runtime, environment, or operating-system
randomness assumptions. Hosts provide source modules, seeds, data, formatter
results, and persistence explicitly. The core has no external dependencies and
forbids unsafe code; the WASM adapter isolates the target allocator.

JavaScript support targets `wasm32-unknown-unknown` through a dependency-light,
handwritten linear-memory ABI and JavaScript/TypeScript wrapper for Deno and
browsers. The WASM adapter supplies its global allocator. A C API is not part of
the initial v2 scope.

The implementation provides a parser with precise spans, an immutable compiled
representation, typed Rust APIs, deterministic seeded sessions, structured
errors, traces, corpus audits, and a formatter boundary. An LSP transport remains
deferred until editor synchronization requirements are known. The runtime
separates immutable grammar content from mutable sampler history so nearby NPCs
can share repetition memory without making every generator globally stateful.

The production core remains `no_std + alloc`, while unit-test harnesses and
integration tests may use `std`. Integration tests load checked-in `.meco`
packages from the filesystem, exercise real imports, and compare exact diagnostics
and deterministic seeded results. Deno and browser harnesses test the compiled
WASM interface.

The full rationale, semantic contract, validation plan, localization boundary,
performance constraints, and implementation phases are in
[V2_SPECIFICATION.md](V2_SPECIFICATION.md).
The formal lexical and strict front-matter grammar is in
[V2_SYNTAX.md](V2_SYNTAX.md).
The host package, WASM ownership/handle, JavaScript error, and CLI stream contracts
are in [V2_INTERFACES.md](V2_INTERFACES.md).
The implementation order and completion gates are tracked in
[ROADMAP.md](ROADMAP.md).

## Project structure

```text
README.md                    V2 overview and canonical syntax corpus
V2_SPECIFICATION.md          Detailed v2 specification and implementation plan
V2_SYNTAX.md                 Normative lexical and complete source grammar
V2_INTERFACES.md             Package, WASM, JavaScript, and CLI contracts
ROADMAP.md                   Phased implementation plan and completion gates
MIGRATION_V1_TO_V2.md        Frozen v1 reader and honest migration contract
COMPATIBILITY.md             Frozen language/runtime/ABI compatibility policy
CONFORMANCE.md               Cross-runtime fixture and release test index
BENCHMARKS.md                Native/WASM workloads and optimization evidence
RELEASE.md                   Distribution release checklist
BYTECODE_FORMAT.md           Normative frozen bytecode/1 layout and policy
BYTECODE_FORMAT_PLAN.md      Completed compiled-artifact and embedding plan
Cargo.toml                   Rust 2024 workspace (MSRV 1.85)
crates/
  mecojoni-benchmarks/       Native operation/allocation workload harness
  mecojoni-cli/              Optional std authoring and migration CLI
  mecojoni-core/             Safe, dependency-free no_std + alloc core
    tests/fixtures/          Filesystem-backed integration corpus
  mecojoni-wasm/             Handwritten WASM ABI and target allocator
js/
  mecojoni.ts                Browser-neutral TypeScript wrapper
  mecojoni_test.ts           Normative Deno integration suite
  browser_smoke.*            Same-artifact browser integration harness
editors/vscode/              TextMate grammar and editor configuration
examples/                    Checked single- and multi-module examples
docs/decisions/              Evidence-backed architecture decisions
v1/
  README.md                  Original runnable v1 documentation
  src/                       V1 compiler, generator, audit, and CLI
  test/                      V1 tests
  assets/                    Mecojoni logo
```

## Current limitations

The implementation currently provides owned UTF-8 sources, dual byte/scalar
spans, ordered multi-error diagnostics, complete source parsing including cooked
block interpolation, immutable multi-module compilation, exact `weighted/1`
generation, iterative graph analysis/expansion, deterministic work limits,
versioned profile records, the composition audit, a version-discovery WASM ABI,
and target-spanning tests. The WASM ABI now includes allocation, package,
compilation, generation, result-copy, diagnostic, opaque-handle, disposal, and
lifecycle-counter operations; the dependency-free TypeScript wrapper is tested in
Deno and Chrome against the same artifact. The executable subset now supports
static and dynamic exact weights, typed scalar/enum request data, guards, typed
rule calls, emitting captures, ordered silent bindings, ordinary references, all
literal/block forms, empty output, public entries, productive recursion, and
opt-in binding/selection traces with exact evaluated weights.
Stable external messages, typed message/input manifests, transitive
complete-message effects, explicit locale fallback, formatter provenance, and
synchronous Rust/JS formatter boundaries are also executable across the shared
Rust/Deno/Chrome corpus. Transactional `diverse/1` sessions, hard/soft cooldown,
bounded exact/edge histories, winner-only commit, and cross-target deterministic
sequences are executable as well. Stable production IDs, exact output provenance,
overlap-only structural/rendered audits, copy-on-write snapshots, and replay
receipts are executable through Rust and the Deno-tested WASM wrapper. The `std`
CLI, frozen v1 migration, conservative formatter, subprocess contract suite, and
initial editor grammar are executable as well. Static trace-off weighted rules use
precomputed cumulative indexes, and production-ID collision checks are
`O(n log n)`; both optimizations have committed native/WASM evidence. A production
Fluent adapter and compound value records are explicitly deferred extensions, not
unimplemented v2 release promises. See [COMPATIBILITY.md](COMPATIBILITY.md),
[CONFORMANCE.md](CONFORMANCE.md), [RELEASE.md](RELEASE.md), the frozen
[bytecode format](BYTECODE_FORMAT.md), and its completed
[implementation plan](BYTECODE_FORMAT_PLAN.md).

## Name

“Mecojoni” is Roman slang loosely conveying “wow.” It was chosen because it is
memorable and fun to say.
