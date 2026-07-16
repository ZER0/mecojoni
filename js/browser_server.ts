const workspace = new URL("../", import.meta.url);
const routes = new Map<string, { path: URL; contentType: string }>([
  ["/", { path: new URL("js/browser_smoke.html", workspace), contentType: "text/html" }],
  [
    "/browser-smoke.js",
    { path: new URL("target/browser-smoke.js", workspace), contentType: "text/javascript" },
  ],
  [
    "/mecojoni.wasm",
    {
      path: new URL("target/wasm32-unknown-unknown/debug/mecojoni_wasm.wasm", workspace),
      contentType: "application/wasm",
    },
  ],
  [
    "/fixtures/weighted/root.meco.md",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/packages/weighted/root.meco.md",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/weighted/common.meco.md",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/packages/weighted/common.meco.md",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/expected/weighted-seeds-v1.outputs",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/expected/weighted-seeds-v1.outputs",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/invalid/root.meco.md",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/packages/compiler-invalid/undefined/root.meco.md",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/milestone5/root.meco.md",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/packages/milestone5/root.meco.md",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/milestone5/common.meco.md",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/packages/milestone5/common.meco.md",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
  [
    "/fixtures/expected/milestone5-seeds-v1.outputs",
    {
      path: new URL(
        "crates/mecojoni-core/tests/fixtures/expected/milestone5-seeds-v1.outputs",
        workspace,
      ),
      contentType: "text/plain; charset=utf-8",
    },
  ],
]);

const port = Number(Deno.args[0] ?? "4517");
Deno.serve({ hostname: "127.0.0.1", port }, async (request) => {
  const route = routes.get(new URL(request.url).pathname);
  if (!route) return new Response("not found", { status: 404 });
  return new Response(await Deno.readFile(route.path), {
    headers: { "content-type": route.contentType },
  });
});
