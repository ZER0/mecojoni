import { Mecojoni } from "./mecojoni.ts";

async function fetchText(path: string): Promise<string> {
  const response = await fetch(path);
  if (!response.ok) throw new Error(`${path} fetch failed: ${response.status}`);
  return await response.text();
}

async function run(): Promise<void> {
  const wasm = await fetch("/mecojoni.wasm").then((response) => {
    if (!response.ok) throw new Error(`WASM fetch failed: ${response.status}`);
    return response.arrayBuffer();
  });
  const meco = await Mecojoni.instantiate(wasm);
  const [root, common, expected] = await Promise.all([
    fetchText("/fixtures/weighted/root.meco.md"),
    fetchText("/fixtures/weighted/common.meco.md"),
    fetchText("/fixtures/expected/weighted-seeds-v1.outputs"),
  ]);
  const compiled = meco.compilePackage({
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco.md",
        source: root,
        resolvedImports: [{ authoredPath: "./common.meco.md", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco.md",
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
      sourceName: "root.meco.md",
      source: await fetchText("/fixtures/invalid/root.meco.md"),
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
    fetchText("/fixtures/milestone5/root.meco.md"),
    fetchText("/fixtures/milestone5/common.meco.md"),
    fetchText("/fixtures/expected/milestone5-seeds-v1.outputs"),
  ]);
  const typed = meco.compilePackage({
    rootId: "root",
    modules: [
      {
        canonicalId: "root",
        sourceId: 0,
        sourceName: "root.meco.md",
        source: typedRoot,
        resolvedImports: [{ authoredPath: "./common.meco.md", targetId: "common" }],
      },
      {
        canonicalId: "common",
        sourceId: 1,
        sourceName: "common.meco.md",
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
  if (meco.liveHandleCount !== 0) throw new Error(`Leaked ${meco.liveHandleCount} handles`);
  document.body.dataset.status = "passed";
  document.body.textContent = "Mecojoni browser WASM weighted and typed corpora passed";
}

try {
  await run();
} catch (error) {
  document.body.dataset.status = "failed";
  document.body.textContent = error instanceof Error ? error.stack ?? error.message : String(error);
}
