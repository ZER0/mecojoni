import type { GenerationLimitOptions, PackageDescription } from "./mecojoni.ts";

export const workloadVersion = "workloads/1";

export const workloadLimits: GenerationLimitOptions = {
  maxDepth: 2_048,
  maxExpansions: 100_000,
  maxOutputScalars: 1_000_000,
  maxOutputBytes: 4_000_000,
  maxSamplerWords: 200_000,
};

export interface Workload {
  name: string;
  class: "realistic" | "adversarial";
  source: string;
  generations: number;
  seedZero: {
    expansions: number;
    samplerWords: number;
    text: string;
  };
}

export function workloads(): Workload[] {
  return [
    {
      name: "flat-64",
      class: "realistic",
      source: flat(64),
      generations: 1_000,
      seedZero: { expansions: 1, samplerWords: 1, text: "alternative-47" },
    },
    {
      name: "tree-dialogue",
      class: "realistic",
      source: tree(),
      generations: 1_000,
      seedZero: {
        expansions: 5,
        samplerWords: 5,
        text: "The pilot catalogued the old radio under the bridge.",
      },
    },
    {
      name: "chain-512",
      class: "adversarial",
      source: chain(512),
      generations: 100,
      seedZero: { expansions: 513, samplerWords: 513, text: "terminal" },
    },
    {
      name: "dense-dag-96x8",
      class: "adversarial",
      source: dense(96, 8),
      generations: 100,
      seedZero: { expansions: 21, samplerWords: 21, text: "terminal" },
    },
    {
      name: "recursive-balanced",
      class: "realistic",
      source: recursive(),
      generations: 1_000,
      seedZero: { expansions: 1, samplerWords: 1, text: "()" },
    },
    {
      name: "fanout-10000",
      class: "adversarial",
      source: flat(10_000),
      generations: 100,
      seedZero: { expansions: 1, samplerWords: 1, text: "alternative-7535" },
    },
  ];
}

export function packageDescription(workload: Workload): PackageDescription {
  return {
    rootId: workload.name,
    modules: [{
      canonicalId: workload.name,
      sourceId: 0,
      sourceName: `${workload.name}.meco.md`,
      source: workload.source,
      resolvedImports: [],
    }],
  };
}

function header(module: string): string {
  return `---\nmeco: 2\nmodule: ${module}\nentry: root\nsampler: weighted/1\nexports: [root]\n---\n\n`;
}

function flat(alternatives: number): string {
  const module = alternatives === 10_000 ? "fanout-10000" : "flat-64";
  let source = `${header(module)}# root\n`;
  for (let index = 0; index < alternatives; index++) source += `- alternative-${index}\n`;
  return source;
}

function tree(): string {
  return header("tree-dialogue") +
    "# root\n- @speaker @action @object @place.\n\n# speaker\n- The pilot\n- A mechanic\n- The courier\n- Our neighbour\n\n# action\n- inspected\n- repaired\n- carried\n- catalogued\n\n# object\n- the old radio\n- a toolkit\n- the package\n- a navigation chart\n\n# place\n- near the workshop\n- beside the market\n- outside the library\n- under the bridge\n";
}

function chain(rules: number): string {
  let source = header("chain-512");
  for (let index = 0; index < rules; index++) {
    source += `# r${index}\n`;
    source += index + 1 === rules ? "- terminal\n\n" : `- @r${index + 1}\n\n`;
  }
  return `${source}# root\n- @r0\n`;
}

function dense(rules: number, width: number): string {
  let source = `${header("dense-dag-96x8")}# root\n- @n0\n\n`;
  for (let index = 0; index < rules; index++) {
    source += `# n${index}\n`;
    const end = Math.min(index + width, rules - 1);
    if (index === rules - 1) source += "- terminal\n\n";
    else {
      for (let target = index + 1; target <= end; target++) source += `- @n${target}\n`;
      source += "\n";
    }
  }
  return source;
}

function recursive(): string {
  return `${header("recursive-balanced")}# root\n- [8] ()\n- [1] (@root)\n- [1] @root@root\n`;
}
