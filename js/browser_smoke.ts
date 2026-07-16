import { type MecoFormatter, Mecojoni } from "./mecojoni.ts";

async function fetchText(path: string): Promise<string> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path} fetch failed: ${response.status}`);
  return await response.text();
}

function parseCatalog(source: string): Map<string, string> {
  return new Map(
    source.trimEnd().split("\n").map((line) => line.split("=", 2) as [string, string]),
  );
}

async function run(): Promise<void> {
  const wasm = await fetch("/mecojoni.wasm").then((response) => {
    if (!response.ok) throw new Error(`WASM fetch failed: ${response.status}`);
    return response.arrayBuffer();
  });
  const meco = await Mecojoni.instantiate(wasm);
  const artifactBytes = await fetch("/fixtures/hello.mecob").then(async (response) => {
    if (!response.ok) throw new Error(`artifact fetch failed: ${response.status}`);
    return new Uint8Array(await response.arrayBuffer());
  });
  const artifactMetadata = meco.inspectArtifact(artifactBytes);
  if (!artifactMetadata.ok) throw new Error(artifactMetadata.error.message);
  if (artifactMetadata.value.version !== "bytecode/1") {
    throw new Error(`unexpected artifact version ${artifactMetadata.value.version}`);
  }
  const artifact = meco.loadArtifact(artifactBytes);
  if (!artifact.ok) throw new Error(artifact.error.message);
  try {
    const generated = meco.generateWeighted(artifact.value, { seed: 7n });
    if (!generated.ok) throw new Error(generated.error.message);
    if (generated.value.text.length === 0) throw new Error("artifact generated empty text");
  } finally {
    artifact.value.dispose();
  }
  const [root, common, expected] = await Promise.all([
    fetchText("/fixtures/weighted/root.meco"),
    fetchText("/fixtures/weighted/common.meco"),
    fetchText("/fixtures/expected/weighted-seeds-v1.outputs"),
  ]);
  const compiled = meco.compilePackage({
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco",
        source: root,
        resolvedImports: [{ authoredPath: "./common.meco", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco",
        source: common,
        resolvedImports: [],
      },
    ],
  });
  if (!compiled.ok) throw new Error(compiled.error.message);
  try {
    const outputs: string[] = [];
    for (let seed = 0n; seed < 16n; seed++) {
      const generated = meco.generateWeighted(compiled.value, { seed });
      if (!generated.ok) throw new Error(generated.error.message);
      outputs.push(
        `${seed}|${generated.value.text}|${generated.value.expansions}|${generated.value.samplerWords}`,
      );
    }
    if (outputs.join("\n") !== expected.trimEnd()) {
      throw new Error("Browser output differs from the Rust and Deno weighted corpus");
    }
  } finally {
    compiled.value.dispose();
  }

  const invalid = meco.compilePackage({
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 7,
      sourceName: "root.meco",
      source: await fetchText("/fixtures/invalid/root.meco"),
      resolvedImports: [],
    }],
  });
  if (invalid.ok) {
    invalid.value.dispose();
    throw new Error("Undefined-rule fixture unexpectedly compiled in the browser");
  }
  if (invalid.diagnostics[0]?.code !== "E_UNDEFINED_RULE") {
    throw new Error(`Unexpected diagnostic: ${invalid.diagnostics[0]?.code}`);
  }
  if (typeof invalid.diagnostics[0]?.span?.start.byte !== "bigint") {
    throw new Error("Browser diagnostic span did not preserve its u64 offset");
  }

  const [typedRoot, typedCommon, typedExpected] = await Promise.all([
    fetchText("/fixtures/milestone5/root.meco"),
    fetchText("/fixtures/milestone5/common.meco"),
    fetchText("/fixtures/expected/milestone5-seeds-v1.outputs"),
  ]);
  const typed = meco.compilePackage({
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco",
        source: typedRoot,
        resolvedImports: [{ authoredPath: "./common.meco", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco",
        source: typedCommon,
        resolvedImports: [],
      },
    ],
  });
  if (!typed.ok) throw new Error(typed.error.message);
  try {
    const typedOutputs: string[] = [];
    for (let seed = 0n; seed < 8n; seed++) {
      const generated = meco.generateWeighted(typed.value, {
        seed,
        data: {
          playerName: { kind: "text", value: "Rin" },
          mood: { kind: "enum", value: "tense" },
          urgency: { kind: "number", numerator: 2n, denominator: 1n },
          enabled: { kind: "boolean", value: true },
        },
        traceBindings: seed === 7n,
        traceSelections: seed === 7n,
      });
      if (!generated.ok) throw new Error(generated.error.message);
      typedOutputs.push(`${seed}|${generated.value.text}`);
      if (seed === 7n && generated.value.bindings.length !== 2) {
        throw new Error("Browser binding trace is incomplete");
      }
      if (
        seed === 7n &&
        generated.value.selections.find((selection) => selection.rule === "scene.alert")?.eligible
            .length !== 2
      ) {
        throw new Error("Browser dynamic-weight trace is incomplete");
      }
    }
    if (typedOutputs.join("\n") !== typedExpected.trimEnd()) {
      throw new Error("Browser typed output differs from the Rust and Deno corpus");
    }
  } finally {
    typed.value.dispose();
  }

  const [localizedRoot, englishCatalog, polishCatalog] = await Promise.all([
    fetchText("/fixtures/milestone6/root.meco"),
    fetchText("/fixtures/milestone6/en.catalog"),
    fetchText("/fixtures/milestone6/pl.catalog"),
  ]);
  const catalogs = new Map([
    ["en", parseCatalog(englishCatalog)],
    ["pl", parseCatalog(polishCatalog)],
  ]);
  const formatter: MecoFormatter = (request) => {
    const actualLocale = [request.requestedLocale, ...request.fallbackLocales].find((locale) =>
      catalogs.has(locale)
    );
    if (actualLocale === undefined) throw new Error("no browser fallback catalog");
    const hero = request.arguments.hero;
    const count = request.arguments.count;
    if (hero?.kind !== "text" || count?.kind !== "number" || count.denominator !== 1n) {
      throw new Error("browser formatter arguments have wrong types");
    }
    const number = Number(count.numerator);
    const mod10 = ((number % 10) + 10) % 10;
    const mod100 = ((number % 100) + 100) % 100;
    const category = actualLocale === "pl"
      ? mod10 === 1 && mod100 !== 11
        ? "one"
        : mod10 >= 2 && mod10 <= 4 && !(mod100 >= 12 && mod100 <= 14)
        ? "few"
        : "many"
      : number === 1
      ? "one"
      : "other";
    const pattern = catalogs.get(actualLocale)?.get(category);
    if (pattern === undefined) throw new Error("browser plural category is absent");
    return {
      text: pattern.replace("{hero}", hero.value).replace("{count}", String(number)),
      actualLocale,
      environmentHash: `fixture/${actualLocale}/v1`,
      workUnits: 1,
      replayable: true,
    };
  };
  const localized = meco.compilePackage({
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 0,
      sourceName: "root.meco",
      source: localizedRoot,
      resolvedImports: [],
    }],
  }, {
    messages: [{
      id: "arrival",
      arguments: [
        { name: "hero", type: { kind: "text" } },
        { name: "count", type: { kind: "number" } },
      ],
    }],
  });
  if (!localized.ok) throw new Error(localized.error.message);
  try {
    for (
      const [locale, count, ending] of [
        ["en", 1n, "arrived with one item."],
        ["pl", 2n, "przybył z 2 przedmiotami."],
        ["pl", 5n, "przybył z 5 przedmiotów."],
      ] as const
    ) {
      const generated = meco.generateWeighted(localized.value, {
        seed: 0n,
        locale,
        formatter,
        data: { itemCount: { kind: "number", numerator: count, denominator: 1n } },
      });
      if (!generated.ok) throw new Error(generated.error.message);
      if (!generated.value.text.endsWith(ending)) {
        throw new Error(`Browser localized category failed for ${locale}/${count}`);
      }
      if (generated.value.message?.actualLocale !== locale) {
        throw new Error("Browser message provenance lost the actual locale");
      }
    }
  } finally {
    localized.value.dispose();
  }

  const [diverseRoot, diverseExpected] = await Promise.all([
    fetchText("/fixtures/milestone7/root.meco"),
    fetchText("/fixtures/expected/milestone7-sequence-v1.outputs"),
  ]);
  const diverse = meco.compilePackage({
    rootId: "root",
    modules: [{
      canonicalId: "root",
      sourceId: 0,
      sourceName: "root.meco",
      source: diverseRoot,
      resolvedImports: [],
    }],
  });
  const session = meco.createSession(0n);
  const repetition = meco.createRepetitionStore();
  if (!diverse.ok) throw new Error(diverse.error.message);
  if (!session.ok) throw new Error(session.error.message);
  if (!repetition.ok) throw new Error(repetition.error.message);
  try {
    const outputs: string[] = [];
    for (let call = 0; call < 16; call++) {
      const generated = meco.generateDiverse(diverse.value, session.value, repetition.value, {
        traceSelections: true,
      });
      if (!generated.ok) throw new Error(generated.error.message);
      outputs.push(
        `${call}|${generated.value.text}|${generated.value.winnerAttempt}|${generated.value.exactRepetitions}|${generated.value.edgeRepetitions}`,
      );
    }
    if (outputs.join("\n") !== diverseExpected.trimEnd()) {
      throw new Error("Browser diverse sequence differs from Rust and Deno");
    }
  } finally {
    diverse.value.dispose();
    session.value.dispose();
    repetition.value.dispose();
  }
  if (meco.liveHandleCount !== 0) throw new Error(`Leaked ${meco.liveHandleCount} handles`);
  document.body.dataset.status = "passed";
  document.body.textContent =
    "Mecojoni browser WASM source, bytecode, typed, localized, and diverse corpora passed";
}

try {
  await run();
} catch (error) {
  document.body.dataset.status = "failed";
  document.body.textContent = error instanceof Error ? error.stack ?? error.message : String(error);
}
