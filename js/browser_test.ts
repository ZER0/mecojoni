const workspace = new URL("../", import.meta.url);

async function existingBrowser(): Promise<string | undefined> {
  const configured = Deno.env.get("MECO_BROWSER");
  if (configured !== undefined) {
    try {
      if ((await Deno.stat(configured)).isFile) return configured;
    } catch {
      throw new Error(`MECO_BROWSER does not name an executable file: ${configured}`);
    }
  }
  const candidates = [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/Applications/Chromium.app/Contents/MacOS/Chromium",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
  ];
  for (const candidate of candidates) {
    try {
      if ((await Deno.stat(candidate)).isFile) return candidate;
    } catch {
      // Try the next known browser location.
    }
  }
  return undefined;
}

const browser = await existingBrowser();

async function waitForDevtools(
  stderr: ReadableStream<Uint8Array>,
): Promise<{ endpoint: string; logs: () => string; drained: Promise<void> }> {
  let logs = "";
  let resolveEndpoint!: (endpoint: string) => void;
  const endpoint = new Promise<string>((resolve) => {
    resolveEndpoint = resolve;
  });
  const drained = (async () => {
    const reader = stderr.getReader();
    const decoder = new TextDecoder();
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      logs += decoder.decode(value, { stream: true });
      const match = logs.match(/DevTools listening on (ws:\/\/[^\s]+)/);
      if (match) resolveEndpoint(match[1]);
    }
    logs += decoder.decode();
  })();
  return {
    endpoint: await Promise.race([
      endpoint,
      new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error(`Chrome DevTools did not start\n${logs}`)), 10_000)
      ),
    ]),
    logs: () => logs,
    drained,
  };
}

async function pageDebuggerUrl(port: string, pageUrl: string): Promise<string> {
  for (let attempt = 0; attempt < 100; attempt++) {
    const targets = await fetch(`http://127.0.0.1:${port}/json/list`).then((response) =>
      response.json()
    ) as Array<{ type: string; url: string; webSocketDebuggerUrl?: string }>;
    const page = targets.find((target) =>
      target.type === "page" && target.url === pageUrl && target.webSocketDebuggerUrl !== undefined
    );
    if (page?.webSocketDebuggerUrl !== undefined) return page.webSocketDebuggerUrl;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("Chrome did not expose the test page through DevTools");
}

async function waitForPageResult(debuggerUrl: string): Promise<void> {
  const socket = new WebSocket(debuggerUrl);
  await new Promise<void>((resolve, reject) => {
    socket.onopen = () => resolve();
    socket.onerror = () => reject(new Error("Chrome DevTools WebSocket failed to open"));
  });
  let nextId = 1;
  const pending = new Map<number, {
    resolve: (value: Record<string, unknown>) => void;
    reject: (error: Error) => void;
  }>();
  socket.onmessage = (event) => {
    const message = JSON.parse(String(event.data)) as {
      id?: number;
      result?: Record<string, unknown>;
      error?: { message: string };
    };
    if (message.id === undefined) return;
    const request = pending.get(message.id);
    if (request === undefined) return;
    pending.delete(message.id);
    if (message.error !== undefined) request.reject(new Error(message.error.message));
    else request.resolve(message.result ?? {});
  };
  const command = (method: string, params: Record<string, unknown> = {}) => {
    const id = nextId++;
    const result = new Promise<Record<string, unknown>>((resolve, reject) => {
      pending.set(id, { resolve, reject });
    });
    socket.send(JSON.stringify({ id, method, params }));
    return result;
  };
  const evaluate = async (expression: string): Promise<unknown> => {
    const response = await command("Runtime.evaluate", { expression, returnByValue: true }) as {
      result?: { value?: unknown };
    };
    return response.result?.value;
  };
  try {
    await command("Runtime.enable");
    for (let attempt = 0; attempt < 200; attempt++) {
      const status = await evaluate("document.body?.dataset.status");
      if (status === "passed") return;
      if (status === "failed") {
        throw new Error(`browser smoke test failed\n${await evaluate("document.body?.innerText")}`);
      }
      await new Promise((resolve) => setTimeout(resolve, 50));
    }
    throw new Error(
      `browser smoke test timed out\n${await evaluate("document.body?.innerText")}`,
    );
  } finally {
    socket.close();
  }
}

Deno.test({
  name: "browser loads the same WASM artifact through the browser-neutral wrapper",
  ignore: browser === undefined,
  fn: async () => {
    const routes = new Map<string, { path: URL; contentType: string }>([
      ["/", { path: new URL("js/browser_smoke.html", workspace), contentType: "text/html" }],
      [
        "/browser-smoke.js",
        { path: new URL("target/browser-smoke.js", workspace), contentType: "text/javascript" },
      ],
      [
        "/mecojoni.wasm",
        {
          path: new URL(
            "target/wasm32-unknown-unknown/debug/mecojoni_wasm.wasm",
            workspace,
          ),
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
      [
        "/fixtures/milestone6/root.meco.md",
        {
          path: new URL(
            "crates/mecojoni-core/tests/fixtures/packages/milestone6/root.meco.md",
            workspace,
          ),
          contentType: "text/plain; charset=utf-8",
        },
      ],
      [
        "/fixtures/milestone6/en.catalog",
        {
          path: new URL(
            "crates/mecojoni-core/tests/fixtures/packages/milestone6/en.catalog",
            workspace,
          ),
          contentType: "text/plain; charset=utf-8",
        },
      ],
      [
        "/fixtures/milestone6/pl.catalog",
        {
          path: new URL(
            "crates/mecojoni-core/tests/fixtures/packages/milestone6/pl.catalog",
            workspace,
          ),
          contentType: "text/plain; charset=utf-8",
        },
      ],
      [
        "/fixtures/milestone7/root.meco.md",
        {
          path: new URL(
            "crates/mecojoni-core/tests/fixtures/packages/milestone7/root.meco.md",
            workspace,
          ),
          contentType: "text/plain; charset=utf-8",
        },
      ],
      [
        "/fixtures/expected/milestone7-sequence-v1.outputs",
        {
          path: new URL(
            "crates/mecojoni-core/tests/fixtures/expected/milestone7-sequence-v1.outputs",
            workspace,
          ),
          contentType: "text/plain; charset=utf-8",
        },
      ],
    ]);
    let port = 0;
    const server = Deno.serve({
      hostname: "127.0.0.1",
      port: 0,
      onListen(address) {
        port = address.port;
      },
    }, async (request) => {
      const route = routes.get(new URL(request.url).pathname);
      if (!route) return new Response("not found", { status: 404 });
      return new Response(await Deno.readFile(route.path), {
        headers: { "content-type": route.contentType },
      });
    });
    let child: Deno.ChildProcess | undefined;
    let stderrDrain: Promise<void> | undefined;
    let browserLogs = () => "";
    try {
      const pageUrl = `http://127.0.0.1:${port}/`;
      const command = new Deno.Command(browser!, {
        args: [
          "--headless=new",
          "--no-sandbox",
          "--disable-gpu",
          "--no-first-run",
          "--no-default-browser-check",
          "--remote-debugging-port=0",
          pageUrl,
        ],
        stdout: "null",
        stderr: "piped",
      });
      child = command.spawn();
      const devtools = await waitForDevtools(child.stderr);
      stderrDrain = devtools.drained;
      browserLogs = devtools.logs;
      const debuggingPort = new URL(devtools.endpoint).port;
      await waitForPageResult(await pageDebuggerUrl(debuggingPort, pageUrl));
    } catch (error) {
      throw new Error(
        `${error instanceof Error ? error.message : String(error)}\n${browserLogs()}`,
      );
    } finally {
      try {
        child?.kill("SIGTERM");
      } catch {
        // Chrome may already have exited after a startup failure.
      }
      await child?.status;
      await stderrDrain;
      await server.shutdown();
    }
  },
});
