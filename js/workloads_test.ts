import { Mecojoni } from "./mecojoni.ts";
import { packageDescription, workloadLimits, workloads, workloadVersion } from "./workloads.ts";

const wasmUrl = new URL(
  "../target/wasm32-unknown-unknown/debug/mecojoni_wasm.wasm",
  import.meta.url,
);

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

function assertEquals<T>(actual: T, expected: T, message: string): void {
  if (!Object.is(actual, expected)) {
    throw new Error(`${message}: expected ${String(expected)}, received ${String(actual)}`);
  }
}

Deno.test("committed workloads match native operation counts and release every WASM allocation", async () => {
  assertEquals(workloadVersion, "workloads/1", "workload version drift");
  const meco = await Mecojoni.instantiate(await Deno.readFile(wasmUrl));
  for (const workload of workloads()) {
    const compiled = meco.compilePackage(packageDescription(workload));
    assert(compiled.ok, compiled.ok ? "" : `${workload.name}: ${compiled.error.message}`);
    assertEquals(meco.liveAllocationCount, 0, `${workload.name}: compile allocation leak`);
    assertEquals(meco.liveAllocationBytes, 0, `${workload.name}: compile byte leak`);
    const generated = meco.generateWeighted(compiled.value, {
      seed: 0n,
      limits: workloadLimits,
    });
    assert(generated.ok, generated.ok ? "" : `${workload.name}: ${generated.error.message}`);
    assertEquals(generated.value.text, workload.seedZero.text, `${workload.name}: text drift`);
    assertEquals(
      generated.value.expansions,
      workload.seedZero.expansions,
      `${workload.name}: expansion drift`,
    );
    assertEquals(
      generated.value.samplerWords,
      workload.seedZero.samplerWords,
      `${workload.name}: sampler drift`,
    );
    const warmMemory = meco.linearMemoryBytes;
    for (let seed = 1; seed <= 8; seed++) {
      const warm = meco.generateWeighted(compiled.value, {
        seed: BigInt(seed),
        limits: workloadLimits,
      });
      assert(warm.ok, warm.ok ? "" : `${workload.name}: ${warm.error.message}`);
    }
    assert(
      meco.linearMemoryBytes <= warmMemory + 65_536,
      `${workload.name}: warm generation grew linear memory by more than one page`,
    );
    compiled.value.dispose();
    assertEquals(meco.liveHandleCount, 0, `${workload.name}: handle leak`);
    assertEquals(meco.liveAllocationCount, 0, `${workload.name}: allocation leak`);
    assertEquals(meco.liveAllocationBytes, 0, `${workload.name}: allocated byte leak`);
  }
});
