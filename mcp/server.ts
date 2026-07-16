// An MCP (Model Context Protocol) server exposing the `meco` CLI as tools for
// any MCP-compatible agent. This is a thin process wrapper: every tool below
// builds an argv array for the real `meco` binary, runs it, and relays its
// `--output jsonl` result. No grammar or compiler logic lives in this file —
// mecojoni-core stays the single source of truth.
//
// See `mcp/README.md` for how to build the CLI first and configure this
// server in specific MCP clients.

import {
  McpServer,
  type RegisteredTool,
} from "npm:@modelcontextprotocol/sdk@^1.29.0/server/mcp.js";
import { StdioServerTransport } from "npm:@modelcontextprotocol/sdk@^1.29.0/server/stdio.js";
import { z } from "npm:zod@^3.25";

/** Directory containing this file's parent, i.e. the repository root. */
export const repoRoot = new URL("..", import.meta.url);

export interface BinaryCandidate {
  path: string;
  mtimeMs: number;
}

/**
 * Picks the most recently built binary from a set of candidates. Always
 * preferring one build profile (e.g. release over debug) risks silently
 * running a stale binary left over from an earlier build after only the
 * other profile was rebuilt — `cargo build -p mecojoni-cli` (debug) and
 * `cargo build -p mecojoni-cli --release` are independent artifacts, and
 * a dev loop that only rebuilds one of them must not be shadowed by the other.
 */
export function pickNewestBinary(candidates: BinaryCandidate[]): string {
  if (candidates.length === 0) {
    throw new Error(
      "no meco binary found under target/{release,debug}/meco; " +
        "run `cargo build -p mecojoni-cli` or set MECO_BIN",
    );
  }
  return [...candidates].sort((left, right) => right.mtimeMs - left.mtimeMs)[0].path;
}

/** Locates the `meco` binary. Honors `MECO_BIN` first; see {@linkcode pickNewestBinary}. */
export function resolveMecoBinary(): string {
  const fromEnv = Deno.env.get("MECO_BIN");
  if (fromEnv) return fromEnv;
  const candidates = ["target/release/meco", "target/debug/meco"]
    .map((relative) => new URL(relative, repoRoot))
    .flatMap((url) => {
      try {
        const info = Deno.statSync(url);
        return info.isFile ? [{ path: url.pathname, mtimeMs: info.mtime?.getTime() ?? 0 }] : [];
      } catch {
        return [];
      }
    });
  return pickNewestBinary(candidates);
}

/**
 * Resolves a source/artifact path an agent supplies against `MECO_PROJECT_ROOT`
 * (or this repository's root, by default) so the server behaves consistently
 * no matter what working directory the MCP client launched it from. Absolute
 * paths pass through unchanged.
 */
export function resolvePath(path: string): string {
  if (path.startsWith("/")) return path;
  const root = Deno.env.get("MECO_PROJECT_ROOT");
  const base = root ? `file://${root.replace(/\/?$/, "/")}` : repoRoot;
  return new URL(path, base).pathname;
}

export interface MecoRun {
  exitCode: number;
  stdout: string;
  stderr: string;
}

/** Runs the `meco` binary with the given arguments and captures its streams. */
export async function runMeco(args: string[]): Promise<MecoRun> {
  const command = new Deno.Command(resolveMecoBinary(), {
    args,
    stdout: "piped",
    stderr: "piped",
  });
  const { code, stdout, stderr } = await command.output();
  return {
    exitCode: code,
    stdout: new TextDecoder().decode(stdout),
    stderr: new TextDecoder().decode(stderr),
  };
}

/** Parses `--output jsonl` stdout into one object per non-empty line. */
export function parseJsonl(stdout: string): unknown[] {
  return stdout
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
    .map((line) => JSON.parse(line));
}

function dataArgs(data: Record<string, string> | undefined): string[] {
  if (!data) return [];
  return Object.entries(data).flatMap(([name, value]) => ["--data", `${name}=${value}`]);
}

/**
 * Runs a `meco` subcommand and turns its result into an MCP tool response.
 * `meco` only writes to stdout on success, so non-empty stdout is treated as
 * success (the exit code is still reported, since e.g. `check --deny-warnings`
 * can succeed with output and still exit 1 to signal warnings). Empty stdout
 * with a nonzero exit code is a real failure — the CLI's own diagnostic on
 * stderr becomes the tool error.
 */
async function runTool(args: string[]) {
  const result = await runMeco(args);
  if (result.stdout.trim().length === 0 && result.exitCode !== 0) {
    return {
      isError: true,
      content: [{
        type: "text" as const,
        text: result.stderr.trim() || `meco exited ${result.exitCode} with no output`,
      }],
    };
  }
  const results = (() => {
    try {
      return parseJsonl(result.stdout);
    } catch {
      return result.stdout;
    }
  })();
  return {
    content: [
      {
        type: "text" as const,
        text: JSON.stringify(
          { exitCode: result.exitCode, results, stderr: result.stderr || undefined },
          null,
          2,
        ),
      },
    ],
  };
}

const seedSchema = z
  .union([
    z.number().int().nonnegative(),
    z.string().regex(/^\d+$/, "seed must be an unsigned integer"),
  ])
  .optional()
  .describe(
    "Deterministic splitmix64 seed (u64). Same seed + grammar + call sequence reproduces exactly.",
  );

const dataSchema = z
  .record(z.string(), z.string())
  .optional()
  .describe(
    'Typed host inputs declared in the grammar\'s `inputs:` front matter, e.g. { playerName: "Ada" }.',
  );

const entrySchema = z.string().optional().describe(
  'Explicit exported qualified rule, e.g. "common.greeting".',
);

// The SDK's `registerTool` cannot infer a tool's argument shape from its Zod
// `inputSchema` unless an `outputSchema` is also supplied (none of these tools
// have one), so each callback below is given an explicit parameter type
// instead of relying on inference.
type Seed = number | string;
interface CheckArgs {
  source: string;
  denyWarnings?: boolean;
  messages?: string;
}
interface GenerateArgs {
  source: string;
  seed?: Seed;
  count?: number;
  entry?: string;
  data?: Record<string, string>;
  messages?: string;
}
interface TraceArgs {
  source: string;
  seed?: Seed;
  count?: number;
  entry?: string;
  data?: Record<string, string>;
}
interface LintArgs {
  source: string;
  denyWarnings?: boolean;
}
interface AuditArgs {
  source: string;
  samples?: number;
  seed?: Seed;
  entry?: string;
  data?: Record<string, string>;
}
interface ManifestArgs {
  source: string;
  messages?: string;
}
interface FmtArgs {
  source: string;
  write?: string;
}
interface BenchArgs {
  source: string;
  count?: number;
  seed?: Seed;
  entry?: string;
  data?: Record<string, string>;
}
interface CompileArtifactArgs {
  source: string;
  write: string;
  messages?: string;
  profile?: "full" | "mapped" | "stripped";
}
interface InspectArtifactArgs {
  artifactPath: string;
}
interface VerifyArtifactArgs {
  artifactPath: string;
}
interface GenerateArtifactArgs {
  artifactPath: string;
  seed?: Seed;
  count?: number;
  entry?: string;
  data?: Record<string, string>;
}

/**
 * Accesses a tool registered on `server` by name, for tests. `McpServer` does
 * not expose a public registry lookup; `private` in its `.d.ts` is a
 * compile-time-only annotation, so this reaches its documented internal
 * `_registeredTools` map rather than re-implementing registration bookkeeping.
 */
export function getRegisteredTool(server: McpServer, name: string): RegisteredTool {
  const registry = (server as unknown as { _registeredTools: Record<string, RegisteredTool> })
    ._registeredTools;
  const tool = registry[name];
  if (!tool) throw new Error(`tool ${name} was not registered`);
  return tool;
}

export function createServer(): McpServer {
  const server = new McpServer({ name: "mecojoni", version: "0.1.0" });

  server.registerTool(
    "meco_check",
    {
      description: "Parse, compile, and validate a .meco package without generating text.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        denyWarnings: z.boolean().optional().describe(
          "Exit 1 if the grammar compiles with warnings.",
        ),
        messages: z.string().optional().describe(
          "Path to a message schema file (id|name:type,... per line).",
        ),
      },
    },
    async ({ source, denyWarnings, messages }: CheckArgs) => {
      const args = ["check", resolvePath(source), "--output", "jsonl"];
      if (denyWarnings) args.push("--deny-warnings");
      if (messages) args.push("--messages", resolvePath(messages));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_generate",
    {
      description: "Generate deterministic weighted text from a compiled .meco package.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        seed: seedSchema,
        count: z.number().int().positive().optional().describe(
          "Number of phrases to generate (default 1).",
        ),
        entry: entrySchema,
        data: dataSchema,
        messages: z.string().optional(),
      },
    },
    async ({ source, seed, count, entry, data, messages }: GenerateArgs) => {
      const args = ["generate", resolvePath(source), "--output", "jsonl"];
      if (seed !== undefined) args.push("--seed", String(seed));
      if (count !== undefined) args.push("--count", String(count));
      if (entry) args.push("--entry", entry);
      if (messages) args.push("--messages", resolvePath(messages));
      args.push(...dataArgs(data));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_trace",
    {
      description:
        "Generate text along with its full derivation trace (selected productions, bindings, provenance).",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        seed: seedSchema,
        count: z.number().int().positive().optional(),
        entry: entrySchema,
        data: dataSchema,
      },
    },
    async ({ source, seed, count, entry, data }: TraceArgs) => {
      const args = ["trace", resolvePath(source), "--output", "jsonl"];
      if (seed !== undefined) args.push("--seed", String(seed));
      if (count !== undefined) args.push("--count", String(count));
      if (entry) args.push("--entry", entry);
      args.push(...dataArgs(data));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_lint",
    {
      description: "Report compiler warnings and composition (fixed-sentence-shell) findings.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        denyWarnings: z.boolean().optional().describe(
          "Exit 1 if any warning or finding is present.",
        ),
      },
    },
    async ({ source, denyWarnings }: LintArgs) => {
      const args = ["lint", resolvePath(source), "--output", "jsonl"];
      if (denyWarnings) args.push("--deny-warnings");
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_audit",
    {
      description: "Sample generations and report structural and rendered repetition findings.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        samples: z.number().int().min(2).optional().describe(
          "Number of samples to draw (default 100).",
        ),
        seed: seedSchema,
        entry: entrySchema,
        data: dataSchema,
      },
    },
    async ({ source, samples, seed, entry, data }: AuditArgs) => {
      const args = ["audit", resolvePath(source), "--output", "jsonl"];
      if (samples !== undefined) args.push("--samples", String(samples));
      if (seed !== undefined) args.push("--seed", String(seed));
      if (entry) args.push("--entry", entry);
      args.push(...dataArgs(data));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_manifest",
    {
      description: "Export a package's compiled input and message schema.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        messages: z.string().optional(),
      },
    },
    async ({ source, messages }: ManifestArgs) => {
      const args = ["manifest", resolvePath(source), "--output", "jsonl"];
      if (messages) args.push("--messages", resolvePath(messages));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_fmt",
    {
      description:
        "Validate and conservatively format .meco source. Without `write`, returns the formatted text.",
      inputSchema: {
        source: z.string().describe("Path to the .meco source file to format."),
        write: z.string().optional().describe("Path to write the formatted source to."),
      },
    },
    async ({ source, write }: FmtArgs) => {
      const args = ["fmt", resolvePath(source), "--output", "jsonl"];
      if (write) args.push("--write", resolvePath(write));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_bench",
    {
      description:
        "Measure deterministic local generation work (expansions, sampler words, elapsed time).",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        count: z.number().int().positive().optional().describe(
          "Number of generations to run (default 1).",
        ),
        seed: seedSchema,
        entry: entrySchema,
        data: dataSchema,
      },
    },
    async ({ source, count, seed, entry, data }: BenchArgs) => {
      const args = ["bench", resolvePath(source), "--output", "jsonl"];
      if (count !== undefined) args.push("--count", String(count));
      if (seed !== undefined) args.push("--seed", String(seed));
      if (entry) args.push("--entry", entry);
      args.push(...dataArgs(data));
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_compile_artifact",
    {
      description:
        "Compile a complete .meco source package into a frozen bytecode/1 (.mecob) artifact.",
      inputSchema: {
        source: z.string().describe("Path to the root .meco source file."),
        write: z.string().describe("Output path for the .mecob artifact."),
        messages: z.string().optional(),
        profile: z.enum(["full", "mapped", "stripped"]).optional().describe(
          "Debug-information profile (default full).",
        ),
      },
    },
    async ({ source, write, messages, profile }: CompileArtifactArgs) => {
      const args = [
        "compile-artifact",
        resolvePath(source),
        "--output",
        "jsonl",
        "--write",
        resolvePath(write),
      ];
      if (messages) args.push("--messages", resolvePath(messages));
      if (profile) args.push("--profile", profile);
      return runTool(args);
    },
  );

  server.registerTool(
    "meco_inspect_artifact",
    {
      description:
        "Report verified metadata (rule/production counts, hashes, entries) for a .mecob artifact.",
      inputSchema: { artifactPath: z.string().describe("Path to a .mecob artifact.") },
    },
    async ({ artifactPath }: InspectArtifactArgs) =>
      runTool(["inspect-artifact", resolvePath(artifactPath), "--output", "jsonl"]),
  );

  server.registerTool(
    "meco_verify_artifact",
    {
      description: "Verify a .mecob artifact's integrity without generating text.",
      inputSchema: { artifactPath: z.string().describe("Path to a .mecob artifact.") },
    },
    async ({ artifactPath }: VerifyArtifactArgs) =>
      runTool(["verify-artifact", resolvePath(artifactPath), "--output", "jsonl"]),
  );

  server.registerTool(
    "meco_generate_artifact",
    {
      description: "Generate deterministic weighted text directly from a verified .mecob artifact.",
      inputSchema: {
        artifactPath: z.string().describe("Path to a .mecob artifact."),
        seed: seedSchema,
        count: z.number().int().positive().optional(),
        entry: entrySchema,
        data: dataSchema,
      },
    },
    async ({ artifactPath, seed, count, entry, data }: GenerateArtifactArgs) => {
      const args = ["generate-artifact", resolvePath(artifactPath), "--output", "jsonl"];
      if (seed !== undefined) args.push("--seed", String(seed));
      if (count !== undefined) args.push("--count", String(count));
      if (entry) args.push("--entry", entry);
      args.push(...dataArgs(data));
      return runTool(args);
    },
  );

  return server;
}

if (import.meta.main) {
  const server = createServer();
  const transport = new StdioServerTransport();
  await server.connect(transport);
}
