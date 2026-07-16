import {
  type MecoFormatter,
  Mecojoni,
  type MessageManifest,
  type PackageDescription,
} from "./mecojoni.ts";

const workspace = new URL("../", import.meta.url);
const wasmUrl = new URL(
  "../target/wasm32-unknown-unknown/debug/mecojoni_wasm.wasm",
  import.meta.url,
);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function assertEquals(actual: unknown, expected: unknown, message = "values differ"): void {
  const left = typeof actual === "string" ? actual : JSON.stringify(actual);
  const right = typeof expected === "string" ? expected : JSON.stringify(expected);
  if (left !== right) throw new Error(`${message}\nactual: ${left}\nexpected: ${right}`);
}

async function instantiate(): Promise<Mecojoni> {
  return await Mecojoni.instantiate(await Deno.readFile(wasmUrl));
}

async function weightedPackage(): Promise<PackageDescription> {
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/weighted/",
    import.meta.url,
  );
  return {
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco.md",
        source: await Deno.readTextFile(new URL("root.meco.md", fixture)),
        resolvedImports: [{ authoredPath: "./common.meco.md", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco.md",
        source: await Deno.readTextFile(new URL("common.meco.md", fixture)),
        resolvedImports: [],
      },
    ],
  };
}

async function milestone5Package(): Promise<PackageDescription> {
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/milestone5/",
    import.meta.url,
  );
  return {
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco.md",
        source: await Deno.readTextFile(new URL("root.meco.md", fixture)),
        resolvedImports: [{ authoredPath: "./common.meco.md", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco.md",
        source: await Deno.readTextFile(new URL("common.meco.md", fixture)),
        resolvedImports: [],
      },
    ],
  };
}

async function milestone6Package(): Promise<PackageDescription> {
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/milestone6/",
    import.meta.url,
  );
  return {
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 0,
      sourceName: "root.meco.md",
      source: await Deno.readTextFile(new URL("root.meco.md", fixture)),
      resolvedImports: [],
    }],
  };
}

async function milestone7Package(): Promise<PackageDescription> {
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/milestone7/root.meco.md",
    import.meta.url,
  );
  return {
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 0,
      sourceName: "root.meco.md",
      source: await Deno.readTextFile(fixture),
      resolvedImports: [],
    }],
  };
}

const milestone6Manifest: MessageManifest = {
  messages: [{
    id: "arrival",
    arguments: [
      { name: "hero", type: { kind: "text" } },
      { name: "count", type: { kind: "number" } },
    ],
  }],
};

Deno.test("Deno compiles and generates the native weighted seed corpus", async () => {
  const meco = await instantiate();
  assertEquals(meco.abiVersion, 1);
  assertEquals(meco.coreApiVersion, 2);
  const compiled = meco.compilePackage(await weightedPackage());
  assert(compiled.ok, compiled.ok ? "" : compiled.error.message);
  try {
    assertEquals(compiled.value.defaultEntry, "weighted.scene");
    assert(compiled.value.entries.includes("weighted.raw-block"), "missing exported entry");
    const outputs: string[] = [];
    for (let seed = 0n; seed < 16n; seed++) {
      const generated = meco.generateWeighted(compiled.value, { seed });
      assert(generated.ok, generated.ok ? "" : generated.error.message);
      outputs.push(
        `${seed}|${generated.value.text}|${generated.value.expansions}|${generated.value.samplerWords}`,
      );
    }
    const expected = await Deno.readTextFile(
      new URL(
        "../crates/mecojoni-core/tests/fixtures/expected/weighted-seeds-v1.outputs",
        import.meta.url,
      ),
    );
    assertEquals(outputs.join("\n"), expected.trimEnd(), "Deno output differs from Rust corpus");
  } finally {
    compiled.value.dispose();
  }
  assertEquals(meco.liveHandleCount, 0, "handles leaked after corpus test");
});

Deno.test("Deno receives structured compiler diagnostics with bigint spans", async () => {
  const meco = await instantiate();
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/compiler-invalid/undefined/root.meco.md",
    import.meta.url,
  );
  const compiled = meco.compilePackage({
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 7,
      sourceName: "root.meco.md",
      source: await Deno.readTextFile(fixture),
      resolvedImports: [],
    }],
  });

  assert(!compiled.ok, "undefined rule unexpectedly compiled");
  assertEquals(compiled.diagnostics[0].code, "E_UNDEFINED_RULE");
  assertEquals(compiled.diagnostics[0].span?.sourceId, 7);
  assert(typeof compiled.diagnostics[0].span?.start.byte === "bigint", "span is not bigint-safe");
  assertEquals(meco.liveHandleCount, 0, "error path leaked handles");
});

Deno.test("Deno executes typed data, guards, dynamic weights, calls, and bindings", async () => {
  const meco = await instantiate();
  const compiled = meco.compilePackage(await milestone5Package());
  assert(compiled.ok, compiled.ok ? "" : compiled.error.message);
  const data = {
    playerName: { kind: "text" as const, value: "Rin" },
    mood: { kind: "enum" as const, value: "tense" },
    urgency: { kind: "number" as const, numerator: 2n, denominator: 1n },
    enabled: { kind: "boolean" as const, value: true },
  };
  try {
    const outputs: string[] = [];
    for (let seed = 0n; seed < 8n; seed++) {
      const generated = meco.generateWeighted(compiled.value, {
        seed,
        data,
        traceBindings: seed === 7n,
        traceSelections: seed === 7n,
      });
      assert(generated.ok, generated.ok ? "" : generated.error.message);
      outputs.push(`${seed}|${generated.value.text}`);
      if (seed === 7n) {
        assertEquals(generated.value.bindings.map((binding) => binding.name), [
          "hero",
          "companion",
        ]);
        assert(
          generated.value.bindings.every((binding) => !binding.emitted),
          "silent bindings were reported as emitted",
        );
        const alert = generated.value.selections.find((selection) =>
          selection.rule === "scene.alert"
        );
        assert(alert !== undefined, "dynamic alert selection trace is missing");
        assertEquals(
          alert.eligible.map((weight) => [
            weight.production,
            weight.baseWeight.numerator.toString(),
            weight.baseWeight.denominator.toString(),
            weight.normalizedWeight.toString(),
          ]),
          [[0, "4", "1", "4"], [1, "1", "1", "1"]],
        );
      }
    }
    const expected = await Deno.readTextFile(
      new URL(
        "../crates/mecojoni-core/tests/fixtures/expected/milestone5-seeds-v1.outputs",
        import.meta.url,
      ),
    );
    assertEquals(outputs.join("\n"), expected.trimEnd());
    const recursive = meco.generateWeighted(compiled.value, {
      entry: "scene.recursion",
      seed: 0n,
      data,
    });
    assert(recursive.ok, recursive.ok ? "" : recursive.error.message);
    assertEquals(recursive.value.text, "inner");
  } finally {
    compiled.value.dispose();
  }
  assertEquals(meco.liveHandleCount, 0);
});

Deno.test("Deno resolves complete messages through the synchronous locale protocol", async () => {
  const meco = await instantiate();
  const fixture = new URL(
    "../crates/mecojoni-core/tests/fixtures/packages/milestone6/",
    import.meta.url,
  );
  const catalogs = new Map<string, Map<string, string>>();
  for (const locale of ["en", "pl"]) {
    const entries = (await Deno.readTextFile(new URL(`${locale}.catalog`, fixture)))
      .trimEnd()
      .split("\n")
      .map((line) => line.split("=", 2) as [string, string]);
    catalogs.set(locale, new Map(entries));
  }
  const formatter: MecoFormatter = (request) => {
    const actualLocale = [request.requestedLocale, ...request.fallbackLocales].find((locale) =>
      catalogs.has(locale)
    );
    if (actualLocale === undefined) throw new Error("no loaded fallback catalog");
    const hero = request.arguments.hero;
    const count = request.arguments.count;
    assert(hero?.kind === "text", "hero formatter argument is not text");
    assert(count?.kind === "number" && count.denominator === 1n, "count is not an integer");
    const number = Number(count.numerator);
    let category: string;
    if (actualLocale === "pl") {
      const mod10 = ((number % 10) + 10) % 10;
      const mod100 = ((number % 100) + 100) % 100;
      category = mod10 === 1 && mod100 !== 11
        ? "one"
        : mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)
        ? "few"
        : "many";
    } else {
      category = number === 1 ? "one" : "other";
    }
    const pattern = catalogs.get(actualLocale)?.get(category);
    assert(pattern !== undefined, "plural category is absent");
    return {
      text: pattern.replace("{hero}", hero.value).replace("{count}", String(number)),
      actualLocale,
      environmentHash: `fixture/${actualLocale}/v1`,
      diagnostics: [],
      workUnits: 1,
      replayable: true,
    };
  };
  const compiled = meco.compilePackage(await milestone6Package(), milestone6Manifest);
  assert(compiled.ok, compiled.ok ? "" : compiled.error.message);
  try {
    for (
      const [locale, count, ending] of [
        ["en", 1n, "arrived with one item."],
        ["en", 2n, "arrived with 2 items."],
        ["pl", 1n, "przybył z jednym przedmiotem."],
        ["pl", 2n, "przybył z 2 przedmiotami."],
        ["pl", 5n, "przybył z 5 przedmiotów."],
      ] as const
    ) {
      const generated = meco.generateWeighted(compiled.value, {
        seed: 0n,
        locale,
        formatter,
        data: { itemCount: { kind: "number", numerator: count, denominator: 1n } },
        traceBindings: true,
      });
      assert(generated.ok, generated.ok ? "" : generated.error.message);
      assert(generated.value.text.endsWith(ending), `${locale}/${count} plural mismatch`);
      assertEquals(generated.value.message?.actualLocale, locale);
      assertEquals(generated.value.bindings[0]?.name, "hero");
    }
    const fallback = meco.generateWeighted(compiled.value, {
      seed: 0n,
      locale: "fr",
      fallbackLocales: ["en"],
      formatter,
      data: { itemCount: { kind: "number", numerator: 1n, denominator: 1n } },
    });
    assert(fallback.ok, fallback.ok ? "" : fallback.error.message);
    assertEquals(fallback.value.message?.requestedLocale, "fr");
    assertEquals(fallback.value.message?.actualLocale, "en");

    const invalidLocale = meco.generateWeighted(compiled.value, {
      seed: 0n,
      locale: "en",
      formatter: () => ({
        text: "wrong locale",
        actualLocale: "de",
        environmentHash: "fixture/de/v1",
        workUnits: 1,
        replayable: true,
      }),
      data: { itemCount: { kind: "number", numerator: 1n, denominator: 1n } },
    });
    assert(!invalidLocale.ok, "formatter escaped the locale chain");
    assertEquals(invalidLocale.diagnostics[0]?.code, "E_LOCALE");
  } finally {
    compiled.value.dispose();
  }
  assertEquals(meco.liveHandleCount, 0);
});

Deno.test("Deno reports message manifest drift before invoking a formatter", async () => {
  const meco = await instantiate();
  const description = await milestone6Package();
  const missing = meco.compilePackage(description, { messages: [] });
  assert(!missing.ok, "missing message unexpectedly compiled");
  assertEquals(missing.diagnostics[0]?.code, "E_MESSAGE_MISSING");

  const drifted = meco.compilePackage(description, {
    messages: [{
      id: "arrival",
      arguments: [
        { name: "hero", type: { kind: "number" } },
        { name: "count", type: { kind: "number" } },
      ],
    }],
  });
  assert(!drifted.ok, "schema drift unexpectedly compiled");
  assertEquals(drifted.diagnostics[0]?.code, "E_MESSAGE_ARGUMENT");
  assertEquals(meco.liveHandleCount, 0);
});

Deno.test("Deno diverse sessions match the transactional Rust sequence", async () => {
  const meco = await instantiate();
  const compiled = meco.compilePackage(await milestone7Package());
  const session = meco.createSession(0n);
  const repetition = meco.createRepetitionStore();
  assert(compiled.ok, compiled.ok ? "" : compiled.error.message);
  assert(session.ok, session.ok ? "" : session.error.message);
  assert(repetition.ok, repetition.ok ? "" : repetition.error.message);
  try {
    const outputs: string[] = [];
    let previous: string | undefined;
    for (let call = 0; call < 16; call++) {
      const generated = meco.generateDiverse(
        compiled.value,
        session.value,
        repetition.value,
        { traceSelections: true },
      );
      assert(generated.ok, generated.ok ? "" : generated.error.message);
      assert(generated.value.text !== previous, `hard gap failed at ${call}`);
      previous = generated.value.text;
      outputs.push(
        `${call}|${generated.value.text}|${generated.value.winnerAttempt}|${generated.value.exactRepetitions}|${generated.value.edgeRepetitions}`,
      );
      assert(
        generated.value.committedRevision === BigInt(call + 1),
        `revision mismatch at ${call}`,
      );
    }
    const expected = await Deno.readTextFile(
      new URL(
        "../crates/mecojoni-core/tests/fixtures/expected/milestone7-sequence-v1.outputs",
        import.meta.url,
      ),
    );
    assertEquals(outputs.join("\n"), expected.trimEnd());
    const cancelled = meco.generateDiverse(compiled.value, session.value, repetition.value, {
      cancelled: true,
    });
    assert(!cancelled.ok, "cancelled diverse call unexpectedly succeeded");
    assertEquals(cancelled.diagnostics[0]?.code, "E_CANCELLED");
    const resumed = meco.generateDiverse(compiled.value, session.value, repetition.value, {});
    assert(resumed.ok, resumed.ok ? "" : resumed.error.message);
    assert(resumed.value.committedRevision === 17n, "cancelled call changed the revision");
  } finally {
    compiled.value.dispose();
    session.value.dispose();
    repetition.value.dispose();
  }
  assertEquals(meco.liveHandleCount, 0);
});

Deno.test("strict JS strings reject unpaired UTF-16 before WASM allocation", async () => {
  const meco = await instantiate();
  const result = meco.createPackage({
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 0,
      sourceName: "broken.meco.md",
      source: "\ud800",
      resolvedImports: [],
    }],
  });

  assert(!result.ok, "unpaired surrogate unexpectedly encoded");
  assertEquals(result.diagnostics[0].code, "E_JS_BOUNDARY");
  assertEquals(meco.liveHandleCount, 0);
});

Deno.test("raw ABI rejects invalid alignment, ranges, and result handles safely", async () => {
  const instantiated = await WebAssembly.instantiate(await Deno.readFile(wasmUrl), {});
  const exports = instantiated.instance.exports as unknown as {
    meco_alloc(length: number, alignment: number): number;
    meco_dealloc(pointer: number, length: number, alignment: number): void;
    meco_call(operation: number, pointer: number, length: number): number;
    meco_result_status(handle: number): number;
    meco_handle_dispose(handle: number): void;
    meco_live_handle_count(): number;
  };

  assertEquals(exports.meco_alloc(8, 3), 0, "non-power-of-two alignment was accepted");
  assertEquals(exports.meco_call(1, 0xffff_fff0, 64), 0, "out-of-range input was accepted");
  assertEquals(exports.meco_result_status(0xffff_ffff), 2, "unknown result looked live");
  exports.meco_handle_dispose(0xffff_ffff);
  const pointer = exports.meco_alloc(8, 8);
  assert(pointer !== 0, "valid raw allocation failed");
  exports.meco_dealloc(pointer, 8, 8);
  exports.meco_dealloc(pointer, 8, 8);
  assertEquals(exports.meco_live_handle_count(), 0);
});

Deno.test("repeated package compile generate dispose cycles release every handle", async () => {
  const meco = await instantiate();
  const description = await weightedPackage();
  let warmMemory = 0;
  for (let cycle = 0; cycle < 100; cycle++) {
    const compiled = meco.compilePackage(description);
    assert(compiled.ok, compiled.ok ? "" : compiled.error.message);
    const generated = meco.generateWeighted(compiled.value, { seed: BigInt(cycle) });
    assert(generated.ok, generated.ok ? "" : generated.error.message);
    compiled.value.dispose();
    compiled.value.dispose();
    assertEquals(meco.liveHandleCount, 0, `handle leak in cycle ${cycle}`);
    if (cycle === 0) warmMemory = meco.linearMemoryBytes;
  }
  assert(
    meco.linearMemoryBytes <= warmMemory + 65_536,
    `linear memory grew from ${warmMemory} to ${meco.linearMemoryBytes} after warmup`,
  );
});

Deno.test("wrapper source remains browser-neutral", async () => {
  const source = await Deno.readTextFile(new URL("mecojoni.ts", import.meta.url));
  assert(!source.includes("Deno."), "browser wrapper contains a Deno runtime dependency");
  assert(
    new URL("js/mecojoni.ts", workspace).pathname.endsWith("/js/mecojoni.ts"),
    "workspace URL failed",
  );
});
