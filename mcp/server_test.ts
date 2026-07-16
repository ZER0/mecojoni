import { assert, assertEquals, assertMatch } from "jsr:@std/assert";
import type { McpServer } from "npm:@modelcontextprotocol/sdk@^1.29.0/server/mcp.js";
import {
  createServer,
  getRegisteredTool,
  parseJsonl,
  pickNewestBinary,
  resolveMecoBinary,
  resolvePath,
  runMeco,
} from "./server.ts";

type ToolResponse = { isError?: boolean; content: { type: "text"; text: string }[] };
type ToolHandler = (input: Record<string, unknown>) => Promise<ToolResponse>;

function toolHandler(server: McpServer, name: string): ToolHandler {
  return getRegisteredTool(server, name).handler as unknown as ToolHandler;
}

Deno.test("resolveMecoBinary finds a built binary on this machine", () => {
  const path = resolveMecoBinary();
  const info = Deno.statSync(path);
  assert(info.isFile, `expected ${path} to be a file`);
});

Deno.test("pickNewestBinary prefers the more recently built profile regardless of order", () => {
  const olderRelease = { path: "/target/release/meco", mtimeMs: 1_000 };
  const newerDebug = { path: "/target/debug/meco", mtimeMs: 2_000 };
  // Regression test: a stale release build must not shadow a freshly rebuilt
  // debug binary just because it happens to be checked first.
  assertEquals(pickNewestBinary([olderRelease, newerDebug]), newerDebug.path);
  assertEquals(pickNewestBinary([newerDebug, olderRelease]), newerDebug.path);
});

Deno.test("pickNewestBinary throws a clear error when no binary exists", () => {
  let threw = false;
  try {
    pickNewestBinary([]);
  } catch (error) {
    threw = true;
    assertMatch((error as Error).message, /cargo build -p mecojoni-cli/);
  }
  assert(threw, "expected pickNewestBinary([]) to throw");
});

Deno.test("resolvePath resolves relative paths against the repository root", () => {
  const resolved = resolvePath("examples/hello.meco");
  assert(resolved.endsWith("examples/hello.meco"));
  assert(Deno.statSync(resolved).isFile);
});

Deno.test("resolvePath passes absolute paths through unchanged", () => {
  assertEquals(resolvePath("/tmp/whatever.meco"), "/tmp/whatever.meco");
});

Deno.test("parseJsonl parses one object per non-empty line", () => {
  const parsed = parseJsonl('{"a":1}\n{"b":2}\n\n');
  assertEquals(parsed, [{ a: 1 }, { b: 2 }]);
});

Deno.test("runMeco surfaces a clean check result for a real fixture", async () => {
  const result = await runMeco(["check", resolvePath("examples/hello.meco"), "--output", "jsonl"]);
  assertEquals(result.exitCode, 0);
  const [report] = parseJsonl(result.stdout) as [{ kind: string; rules: number }];
  assertEquals(report.kind, "check");
  assert(report.rules > 0);
});

Deno.test("meco_check tool reports a real fixture as valid", async () => {
  const server = createServer();
  const handler = toolHandler(server, "meco_check");
  const response = await handler({ source: "examples/hello.meco" });
  assert(!response.isError, response.content[0]?.text);
  const body = JSON.parse(response.content[0].text);
  assertEquals(body.exitCode, 0);
  assertEquals(body.results[0].kind, "check");
});

Deno.test("meco_generate tool is deterministic for a fixed seed", async () => {
  const server = createServer();
  const handler = toolHandler(server, "meco_generate");
  const first = await handler({ source: "examples/hello.meco", seed: 42, count: 3 });
  const second = await handler({ source: "examples/hello.meco", seed: 42, count: 3 });
  assertEquals(first.content[0].text, second.content[0].text);
  const body = JSON.parse(first.content[0].text);
  assertEquals(body.results.length, 3);
  for (const result of body.results) {
    assertEquals(result.kind, "generation");
    assert(typeof result.text === "string" && result.text.length > 0);
  }
});

Deno.test("meco_generate tool threads typed host data into an imported package", async () => {
  const server = createServer();
  const handler = toolHandler(server, "meco_generate");
  const response = await handler({
    source: "examples/npc/root.meco",
    seed: 1,
    data: { playerName: "Rin" },
  });
  assert(!response.isError, response.content[0]?.text);
  const body = JSON.parse(response.content[0].text);
  assertMatch(body.results[0].text, /^Rin,/);
});

Deno.test("meco_generate tool reports a missing required input as a clean tool error", async () => {
  const server = createServer();
  const handler = toolHandler(server, "meco_generate");
  const response = await handler({ source: "examples/npc/root.meco", seed: 1 });
  assert(response.isError, "expected a missing required input to be a tool error");
  assertMatch(response.content[0].text, /playerName/);
});

Deno.test("meco_manifest tool reports the typed inputs a package declares", async () => {
  const server = createServer();
  const handler = toolHandler(server, "meco_manifest");
  const response = await handler({ source: "examples/npc/root.meco" });
  const body = JSON.parse(response.content[0].text);
  assertEquals(body.results[0].inputs, [{ name: "playerName", type: "text" }]);
});
