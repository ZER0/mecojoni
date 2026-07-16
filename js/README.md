# Mecojoni WebAssembly wrapper

`mecojoni.ts` is a dependency-free, browser-neutral wrapper for `meco-wasm/1`. It accepts package
source strings, rejects unpaired UTF-16 before encoding, copies strict UTF-8 through explicit WASM
allocations, and exposes ordinary compiler or generation failures as `MecoResult<T>` values. Seeds
and diagnostic span offsets use `bigint` so JavaScript never rounds a Rust `u64`.

Generation data uses explicit discriminated `MecoValue` objects. Exact numbers carry `bigint`
numerator/denominator fields; finite enums carry their member string and are checked against the
compiled schema. `traceBindings: true` returns ordered silent/emitting binding records;
`traceSelections: true` returns exact rational and normalized weights for replay inspection.

Complete messages are compiled against an explicit typed manifest and resolved by a synchronous
callback over resources the application has already loaded:

```ts
const grammar = meco.compilePackage(description, {
  messages: [{
    id: "arrival",
    arguments: [
      { name: "hero", type: { kind: "text" } },
      { name: "count", type: { kind: "number" } },
    ],
  }],
});
if (!grammar.ok) throw new Error(grammar.error.message);

const generated = meco.generateWeighted(grammar.value, {
  seed: 7n,
  locale: "pl",
  fallbackLocales: ["en"],
  data: { itemCount: { kind: "number", numerator: 2n, denominator: 1n } },
  formatter(request) {
    return {
      text: renderFromPreloadedCatalog(request),
      actualLocale: "pl",
      environmentHash: "catalogs-2026-07-16/formatter-1",
      diagnostics: [],
      workUnits: 1,
      replayable: true,
    };
  },
});
```

The callback cannot return a promise or perform deferred I/O. Its `actualLocale` must be the
requested locale or a member of the ordered fallback chain. Fatal diagnostics, more than 10,000
reported work units, invalid replay provenance, and final scalar/UTF-8 output-limit violations fail
the whole request without partial text. `GenerationOutput.message` retains coarse message and locale
provenance; `formatterDiagnostics` retains successful formatter warnings.

Build and run the normative Deno integration suite from the repository root:

```sh
deno task js:check
deno task wasm:test
```

The browser test bundles that same wrapper, serves the same debug WASM artifact and checked-in
fixtures from a temporary Deno server, and runs the Rust/Deno seed corpus plus structured-diagnostic
checks in headless Chrome when Chrome is available:

```sh
deno task wasm:browser:test
```

Package and grammar objects own opaque handles and provide idempotent `dispose()`. Applications
should dispose grammars in `finally` blocks. Result handles and temporary linear-memory buffers are
always disposed internally. The allocator may retain its high-water memory pages for reuse; the
lifecycle test warms it once, runs 100 compile/generate/dispose cycles, requires zero live handles
after every cycle, and permits at most one additional 64 KiB page after warm-up.

The currently executable language subset is documented in the root README. Typed request data,
guards, dynamic weights, calls, captures, bindings, complete-message manifests, explicit locale
fallback, and synchronous formatter callbacks execute in Rust, Deno, and Chrome.
