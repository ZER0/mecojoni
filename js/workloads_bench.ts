import { Mecojoni, type MessageManifest, type PackageDescription } from "./mecojoni.ts";
import { packageDescription, workloadLimits, workloads, workloadVersion } from "./workloads.ts";

const wasmUrl = new URL(
  "../target/wasm32-unknown-unknown/release/mecojoni_wasm.wasm",
  import.meta.url,
);
const wasm = await Deno.readFile(wasmUrl);
const samples = 5;
const encoder = new TextEncoder();

function median(values: number[]): number {
  return values.toSorted((left, right) => left - right)[Math.floor(values.length / 2)];
}

for (const workload of workloads()) {
  const compileMs: number[] = [];
  const generationMs: number[] = [];
  let evidence: Record<string, number> | undefined;
  for (let sample = 0; sample < samples; sample++) {
    const meco = await Mecojoni.instantiate(wasm);
    const memoryBefore = meco.linearMemoryBytes;
    const compileStarted = performance.now();
    const compiled = meco.compilePackage(packageDescription(workload));
    if (!compiled.ok) throw new Error(`${workload.name}: ${compiled.error.message}`);
    compileMs.push(performance.now() - compileStarted);
    const memoryAfterCompile = meco.linearMemoryBytes;
    const generationStarted = performance.now();
    let expansions = 0;
    let samplerWords = 0;
    let outputBytes = 0;
    for (let seed = 0; seed < workload.generations; seed++) {
      const result = meco.generateWeighted(compiled.value, {
        seed: BigInt(seed),
        limits: workloadLimits,
      });
      if (!result.ok) throw new Error(`${workload.name}: ${result.error.message}`);
      expansions += result.value.expansions;
      samplerWords += result.value.samplerWords;
      outputBytes += encoder.encode(result.value.text).byteLength;
    }
    generationMs.push(performance.now() - generationStarted);
    compiled.value.dispose();
    if (meco.liveHandleCount !== 0 || meco.liveAllocationCount !== 0) {
      throw new Error(`${workload.name}: WASM benchmark leaked host resources`);
    }
    evidence ??= {
      expansions,
      samplerWords,
      outputBytes,
      memoryBefore,
      memoryAfterCompile,
      memoryAfterDispose: meco.linearMemoryBytes,
    };
  }
  console.log(JSON.stringify({
    engine: "v2-wasm",
    version: workloadVersion,
    scenario: workload.name,
    class: workload.class,
    samples,
    sourceBytes: encoder.encode(workload.source).byteLength,
    generations: workload.generations,
    compileMsMedian: median(compileMs),
    generationMsMedian: median(generationMs),
    ...evidence,
    liveHandles: 0,
    liveAllocations: 0,
    liveAllocationBytes: 0,
  }));

  const artifact = await Deno.readFile(
    new URL(`../benchmarks/artifacts/workloads/${workload.name}.mecob`, import.meta.url),
  );
  const loadMs: number[] = [];
  const artifactGenerationMs: number[] = [];
  let artifactEvidence: Record<string, number> | undefined;
  for (let sample = 0; sample < samples; sample++) {
    const meco = await Mecojoni.instantiate(wasm);
    const memoryBefore = meco.linearMemoryBytes;
    const loadStarted = performance.now();
    const loaded = meco.loadArtifact(artifact);
    if (!loaded.ok) throw new Error(`${workload.name} artifact: ${loaded.error.message}`);
    loadMs.push(performance.now() - loadStarted);
    const memoryAfterLoad = meco.linearMemoryBytes;
    const generationStarted = performance.now();
    let expansions = 0;
    let samplerWords = 0;
    let outputBytes = 0;
    for (let seed = 0; seed < workload.generations; seed++) {
      const result = meco.generateWeighted(loaded.value, {
        seed: BigInt(seed),
        limits: workloadLimits,
      });
      if (!result.ok) throw new Error(`${workload.name} artifact: ${result.error.message}`);
      expansions += result.value.expansions;
      samplerWords += result.value.samplerWords;
      outputBytes += encoder.encode(result.value.text).byteLength;
    }
    artifactGenerationMs.push(performance.now() - generationStarted);
    loaded.value.dispose();
    if (meco.liveHandleCount !== 0 || meco.liveAllocationCount !== 0) {
      throw new Error(`${workload.name} artifact: WASM benchmark leaked host resources`);
    }
    artifactEvidence ??= {
      expansions,
      samplerWords,
      outputBytes,
      memoryBefore,
      memoryAfterLoad,
      memoryAfterDispose: meco.linearMemoryBytes,
    };
  }
  console.log(JSON.stringify({
    engine: "v2-wasm-bytecode",
    version: workloadVersion,
    scenario: workload.name,
    class: workload.class,
    samples,
    artifactBytes: artifact.byteLength,
    generations: workload.generations,
    loadMsMedian: median(loadMs),
    generationMsMedian: median(artifactGenerationMs),
    ...artifactEvidence,
    liveHandles: 0,
    liveAllocations: 0,
    liveAllocationBytes: 0,
  }));
}

const harborDirectory = new URL("../benchmarks/packages/harbor/", import.meta.url);
const [root, cast, scenes] = await Promise.all([
  Deno.readTextFile(new URL("root.meco", harborDirectory)),
  Deno.readTextFile(new URL("cast.meco", harborDirectory)),
  Deno.readTextFile(new URL("scenes.meco", harborDirectory)),
]);
const harbor: PackageDescription = {
  rootId: "harbor",
  modules: [
    {
      canonicalId: "harbor",
      sourceId: 0,
      sourceName: "root.meco",
      source: root,
      resolvedImports: [
        { authoredPath: "./cast.meco", targetId: "cast" },
        { authoredPath: "./scenes.meco", targetId: "scenes" },
      ],
    },
    {
      canonicalId: "cast",
      sourceId: 1,
      sourceName: "cast.meco",
      source: cast,
      resolvedImports: [],
    },
    {
      canonicalId: "scenes",
      sourceId: 2,
      sourceName: "scenes.meco",
      source: scenes,
      resolvedImports: [],
    },
  ],
};
const harborManifest: MessageManifest = {
  messages: [{ id: "harbor-welcome", arguments: [{ name: "visitor", type: { kind: "text" } }] }],
};
const harborCompileMs: number[] = [];
const harborGenerationMs: number[] = [];
let harborEvidence: Record<string, number> | undefined;
for (let sample = 0; sample < samples; sample++) {
  const meco = await Mecojoni.instantiate(wasm);
  const memoryBefore = meco.linearMemoryBytes;
  const started = performance.now();
  const compiled = meco.compilePackage(harbor, harborManifest);
  if (!compiled.ok) throw new Error(`harbor: ${compiled.error.message}`);
  harborCompileMs.push(performance.now() - started);
  const memoryAfterCompile = meco.linearMemoryBytes;
  const generationStarted = performance.now();
  const result = meco.generateWeighted(compiled.value, {
    entry: "harbor.scene",
    seed: 0n,
    limits: workloadLimits,
    data: {
      visitor: { kind: "text", value: "Rin" },
      mood: { kind: "enum", value: "tense" },
      urgency: { kind: "number", numerator: 1n, denominator: 1n },
    },
  });
  harborGenerationMs.push(performance.now() - generationStarted);
  if (!result.ok) throw new Error(`harbor: ${result.error.message}`);
  compiled.value.dispose();
  if (meco.liveHandleCount !== 0 || meco.liveAllocationCount !== 0) {
    throw new Error("harbor: WASM benchmark leaked host resources");
  }
  harborEvidence ??= {
    expansions: result.value.expansions,
    samplerWords: result.value.samplerWords,
    outputBytes: encoder.encode(result.value.text).byteLength,
    memoryBefore,
    memoryAfterCompile,
    memoryAfterDispose: meco.linearMemoryBytes,
  };
}
console.log(JSON.stringify({
  engine: "v2-wasm",
  version: "startup/1",
  scenario: "harbor-dialogue",
  class: "representative",
  samples,
  sourceBytes: encoder.encode(root + cast + scenes).byteLength,
  generations: 1,
  compileMsMedian: median(harborCompileMs),
  generationMsMedian: median(harborGenerationMs),
  ...harborEvidence,
  liveHandles: 0,
  liveAllocations: 0,
  liveAllocationBytes: 0,
}));

const harborArtifact = await Deno.readFile(
  new URL("../benchmarks/artifacts/harbor.mecob", import.meta.url),
);
const harborLoadMs: number[] = [];
const harborArtifactGenerationMs: number[] = [];
let harborArtifactEvidence: Record<string, number> | undefined;
for (let sample = 0; sample < samples; sample++) {
  const meco = await Mecojoni.instantiate(wasm);
  const memoryBefore = meco.linearMemoryBytes;
  const started = performance.now();
  const loaded = meco.loadArtifact(harborArtifact);
  if (!loaded.ok) throw new Error(`harbor artifact: ${loaded.error.message}`);
  harborLoadMs.push(performance.now() - started);
  const memoryAfterLoad = meco.linearMemoryBytes;
  const generationStarted = performance.now();
  const result = meco.generateWeighted(loaded.value, {
    entry: "harbor.scene",
    seed: 0n,
    limits: workloadLimits,
    data: {
      visitor: { kind: "text", value: "Rin" },
      mood: { kind: "enum", value: "tense" },
      urgency: { kind: "number", numerator: 1n, denominator: 1n },
    },
  });
  harborArtifactGenerationMs.push(performance.now() - generationStarted);
  if (!result.ok) throw new Error(`harbor artifact: ${result.error.message}`);
  loaded.value.dispose();
  if (meco.liveHandleCount !== 0 || meco.liveAllocationCount !== 0) {
    throw new Error("harbor artifact: WASM benchmark leaked host resources");
  }
  harborArtifactEvidence ??= {
    expansions: result.value.expansions,
    samplerWords: result.value.samplerWords,
    outputBytes: encoder.encode(result.value.text).byteLength,
    memoryBefore,
    memoryAfterLoad,
    memoryAfterDispose: meco.linearMemoryBytes,
  };
}
console.log(JSON.stringify({
  engine: "v2-wasm-bytecode",
  version: "startup/1",
  scenario: "harbor-dialogue",
  class: "representative",
  samples,
  artifactBytes: harborArtifact.byteLength,
  generations: 1,
  loadMsMedian: median(harborLoadMs),
  generationMsMedian: median(harborArtifactGenerationMs),
  ...harborArtifactEvidence,
  liveHandles: 0,
  liveAllocations: 0,
  liveAllocationBytes: 0,
}));
