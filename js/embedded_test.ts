import { Mecojoni } from "./mecojoni.ts";

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) throw new Error(message);
}

Deno.test("content-specific WASM opens its embedded multi-module grammar", async () => {
  const wasm = await Deno.readFile(
    new URL("../target/embedded/wasm32-unknown-unknown/debug/mecojoni_wasm.wasm", import.meta.url),
  );
  const meco = await Mecojoni.instantiate(wasm);
  const embedded = meco.openEmbeddedGrammar();
  assert(embedded.ok, embedded.ok ? "" : embedded.error.message);
  const external = meco.loadArtifact(
    await Deno.readFile(new URL("../benchmarks/artifacts/harbor.mecob", import.meta.url)),
  );
  assert(external.ok, external.ok ? "" : external.error.message);
  try {
    assert(embedded.value.entries.includes("harbor.scene"), "embedded Harbor entries are absent");
    const generated = meco.generateWeighted(embedded.value, {
      entry: "harbor.scene",
      seed: 0n,
      data: {
        visitor: { kind: "text", value: "Rin" },
        mood: { kind: "enum", value: "tense" },
        urgency: { kind: "number", numerator: 1n, denominator: 1n },
      },
    });
    assert(generated.ok, generated.ok ? "" : generated.error.message);
    assert(generated.value.expansions === 4, "embedded operation contract changed");
    assert(generated.value.samplerWords === 4, "embedded sampler contract changed");
    const externalGenerated = meco.generateWeighted(external.value, {
      entry: "harbor.scene",
      seed: 0n,
      data: {
        visitor: { kind: "text", value: "Rin" },
        mood: { kind: "enum", value: "tense" },
        urgency: { kind: "number", numerator: 1n, denominator: 1n },
      },
    });
    assert(externalGenerated.ok, externalGenerated.ok ? "" : externalGenerated.error.message);
    assert(
      JSON.stringify(generated.value) === JSON.stringify(externalGenerated.value),
      "embedded and external artifact outputs differ",
    );
  } finally {
    embedded.value.dispose();
    external.value.dispose();
  }
  assert(meco.liveHandleCount === 0, "embedded grammar handle leaked");
  assert(meco.liveAllocationCount === 0, "embedded ABI allocation leaked");
});
