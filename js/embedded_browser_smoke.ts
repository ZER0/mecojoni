import { Mecojoni } from "./mecojoni.ts";

try {
  const response = await fetch("/mecojoni.wasm");
  if (!response.ok) throw new Error(`embedded WASM fetch failed: ${response.status}`);
  const meco = await Mecojoni.instantiate(await response.arrayBuffer());
  const grammar = meco.openEmbeddedGrammar();
  if (!grammar.ok) throw new Error(grammar.error.message);
  try {
    const generated = meco.generateWeighted(grammar.value, {
      entry: "harbor.scene",
      seed: 0n,
      data: {
        visitor: { kind: "text", value: "Rin" },
        mood: { kind: "enum", value: "tense" },
        urgency: { kind: "number", numerator: 1n, denominator: 1n },
      },
    });
    if (!generated.ok) throw new Error(generated.error.message);
    if (generated.value.expansions !== 4 || generated.value.samplerWords !== 4) {
      throw new Error("embedded browser operation contract changed");
    }
  } finally {
    grammar.value.dispose();
  }
  if (meco.liveHandleCount !== 0 || meco.liveAllocationCount !== 0) {
    throw new Error("embedded browser execution leaked resources");
  }
  document.body.dataset.status = "passed";
  document.body.textContent = "Embedded Harbor grammar opened without a content fetch";
} catch (error) {
  document.body.dataset.status = "failed";
  document.body.textContent = error instanceof Error ? error.stack ?? error.message : String(error);
}
