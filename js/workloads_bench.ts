import { Mecojoni } from "./mecojoni.ts";
import { packageDescription, workloadLimits, workloads, workloadVersion } from "./workloads.ts";

const wasmUrl = new URL(
  "../target/wasm32-unknown-unknown/release/mecojoni_wasm.wasm",
  import.meta.url,
);
const meco = await Mecojoni.instantiate(await Deno.readFile(wasmUrl));

for (const workload of workloads()) {
  const memoryBefore = meco.linearMemoryBytes;
  const compileStarted = performance.now();
  const compiled = meco.compilePackage(packageDescription(workload));
  if (!compiled.ok) throw new Error(`${workload.name}: ${compiled.error.message}`);
  const compileMs = performance.now() - compileStarted;
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
    outputBytes += new TextEncoder().encode(result.value.text).byteLength;
  }
  const generationMs = performance.now() - generationStarted;
  compiled.value.dispose();
  const memoryAfterDispose = meco.linearMemoryBytes;
  console.log(JSON.stringify({
    version: workloadVersion,
    scenario: workload.name,
    class: workload.class,
    sourceBytes: new TextEncoder().encode(workload.source).byteLength,
    generations: workload.generations,
    compileMs,
    generationMs,
    expansions,
    samplerWords,
    outputBytes,
    memoryBefore,
    memoryAfterCompile,
    memoryAfterDispose,
    liveHandles: meco.liveHandleCount,
    liveAllocations: meco.liveAllocationCount,
    liveAllocationBytes: meco.liveAllocationBytes,
  }));
}
