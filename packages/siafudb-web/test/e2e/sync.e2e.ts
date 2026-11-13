/**
 * Device-to-device sync, end to end: two browser contexts (separate storage),
 * each with its own database, converge through a real `gsp-relay` process
 * that stores only ciphertext.
 *
 * The relay binary is not part of this repository. Build it from the
 * protocol repository at the commit pinned in the workspace `Cargo.toml`
 * (`cargo build -p gsp-relay --release`) and point `GSP_RELAY_BIN` at it.
 * Without the variable the usual local build locations are tried. If no
 * binary is found the suite is skipped with a warning on a developer's
 * machine and FAILS in CI (`CI` set), as it does when `GSP_RELAY_BIN` names
 * a file that does not exist.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
  cpSync,
  createReadStream,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
} from "node:fs";
import http from "node:http";
import type { AddressInfo } from "node:net";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  chromium,
  type Browser,
  type BrowserContext,
  type Page,
} from "playwright";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { SiafuDB, SyncStatus } from "../../src/index.js";

const root = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../..",
);
const MIME: Record<string, string> = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".wasm": "application/wasm",
  ".map": "application/json",
};
/** Written as a property value; must never reach the relay's disk in the clear. */
const MARKER = "RELAY-MUST-NOT-SEE-OKAPI-90417";
/** How long convergence may take before a step fails. */
const WAIT_MS = 30_000;

function findRelay(): string | null {
  const explicit = process.env.GSP_RELAY_BIN;
  if (explicit) {
    if (!existsSync(explicit))
      throw new Error(`GSP_RELAY_BIN does not exist: ${explicit}`);
    return explicit;
  }
  const candidates = [
    "/tmp/siafu-target-gsp/release/gsp-relay",
    path.resolve(root, "../../../siafudb-gsp/target/release/gsp-relay"),
  ];
  return candidates.find((c) => existsSync(c)) ?? null;
}

const relayBin = findRelay();
if (!relayBin && process.env.CI)
  throw new Error(
    "sync.e2e: no gsp-relay binary. In CI this suite must run: build the relay and set GSP_RELAY_BIN.",
  );
if (!relayBin)
  console.warn(
    "sync.e2e: SKIPPED, no gsp-relay binary found. Build it from the protocol repository and set GSP_RELAY_BIN.",
  );

let server: http.Server;
let origin = "";
let browser: Browser;
let relayUrl = "";
let relayDir = "";
let relayLog = "";
let scratch = "";
/** Everything the pages and their workers printed. */
const consoleLines: string[] = [];
/** Relay processes to stop at the end. */
const relays = new Set<ChildProcess>();
let mainRelay: ChildProcess;

/**
 * Start a relay; resolves to the process and its base URL. `port` 0 picks a
 * free one. `registrationToken` goes through the environment, as the relay's
 * documentation recommends.
 */
function startRelay(
  dataDir: string,
  port = 0,
  registrationToken?: string,
): Promise<{ child: ChildProcess; url: string }> {
  return new Promise((resolve, reject) => {
    const env = { ...process.env };
    delete env.GSP_RELAY_REGISTRATION_TOKEN;
    if (registrationToken !== undefined)
      env.GSP_RELAY_REGISTRATION_TOKEN = registrationToken;
    const child = spawn(
      relayBin!,
      ["--addr", `127.0.0.1:${port}`, "--data-dir", dataDir],
      { stdio: ["ignore", "ignore", "pipe"], env },
    );
    relays.add(child);
    let log = "";
    const timer = setTimeout(
      () => reject(new Error("gsp-relay did not start within 20 s")),
      20_000,
    );
    child.stderr!.setEncoding("utf8");
    child.stderr!.on("data", (chunk: string) => {
      log += chunk;
      relayLog += chunk;
      const m = /listening on (http:\/\/127\.0\.0\.1:\d+)/.exec(log);
      if (m) {
        clearTimeout(timer);
        resolve({ child, url: m[1]! });
      }
    });
    child.once("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      relays.delete(child);
      reject(new Error(`gsp-relay exited early with code ${code}: ${log}`));
    });
  });
}

async function stopRelay(child: ChildProcess): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const gone = new Promise((r) => child.once("exit", r));
  child.kill("SIGTERM");
  await gone;
}

beforeAll(async () => {
  if (!relayBin) return;
  server = http.createServer((req, res) => {
    const rel = decodeURIComponent((req.url ?? "/").split("?")[0]!);
    const file = path.join(
      root,
      rel === "/" ? "/test/e2e/site/index.html" : rel,
    );
    if (!file.startsWith(root) || !existsSync(file) || file === root) {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, {
      "content-type": MIME[path.extname(file)] ?? "application/octet-stream",
      "cache-control": "no-store",
    });
    createReadStream(file).pipe(res);
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  scratch = mkdtempSync(path.join(tmpdir(), "siafudb-sync-e2e-"));
  relayDir = path.join(scratch, "relay");
  mkdirSync(relayDir);
  const started = await startRelay(relayDir);
  mainRelay = started.child;
  relayUrl = started.url;
  browser = await chromium.launch();
});

afterAll(async () => {
  await browser?.close();
  server?.close();
  for (const child of [...relays]) await stopRelay(child);
  if (scratch) rmSync(scratch, { recursive: true, force: true });
});

/** One device: its own browser context (own OPFS, IndexedDB, locks) and page. */
interface Device {
  label: string;
  ctx: BrowserContext;
  page: Page;
}

type Win = {
  db: SiafuDB;
  remote: { events: number; changes: number };
  siafuReady: Promise<{ SiafuDB: typeof SiafuDB }>;
};

async function newDevice(label: string, ctx?: BrowserContext): Promise<Device> {
  ctx ??= await browser.newContext();
  const page = await ctx.newPage();
  const note = (text: string) => consoleLines.push(`[${label}] ${text}`);
  page.on("pageerror", (e) => note(`pageerror ${e.message}`));
  page.on("console", (m) => note(m.text()));
  page.on("worker", (w) => w.on("console", (m) => note(m.text())));
  await page.goto(`${origin}/test/e2e/site/index.html`);
  return { label, ctx, page };
}

/** Open the database `name` as `window.db` and count remote-change events. */
async function openDb(
  d: Device,
  name: string,
  options: { workerUrl?: string } = {},
) {
  return d.page.evaluate(
    async ({ name, options }) => {
      const w = window as unknown as Win;
      const { SiafuDB } = await w.siafuReady;
      const db = await SiafuDB.open(name, { autosaveMs: 50, ...options });
      w.db = db;
      w.remote = { events: 0, changes: 0 };
      db.sync.onRemoteChange((e) => {
        w.remote.events += 1;
        w.remote.changes += e.changes;
      });
      return {
        restored: db.restored,
        backend: db.backend,
        status: await db.sync.status(),
      };
    },
    { name, options },
  );
}

const join = (d: Device, code: string, url = relayUrl) =>
  d.page.evaluate(
    ({ url, code }) =>
      (window as unknown as Win).db.sync.join({ relayUrl: url, code }),
    { url, code },
  );

const syncNow = (d: Device) =>
  d.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());

const keys = async (d: Device, label = "Note") =>
  (
    (await rows(d, `MATCH (n:${label}) RETURN n.key ORDER BY n.key`)) as [
      string,
    ][]
  ).map((r) => r[0]);

/** Call a `db.sync` method that should reject; resolves to `code: message`. */
const failure = (
  d: Device,
  method: "create" | "join" | "rejoinAsNewDevice" | "syncNow",
  arg?: Record<string, unknown>,
) =>
  d.page.evaluate(
    ({ method, arg }) =>
      (
        (window as unknown as Win).db.sync[method] as (
          a?: unknown,
        ) => Promise<unknown>
      )(arg).then(
        () => "resolved",
        (e: Error & { code?: string }) => `${e.code}: ${e.message}`,
      ),
    { method, arg },
  );

const run = (d: Device, query: string, params: Record<string, unknown> = {}) =>
  d.page.evaluate(
    ({ query, params }) =>
      (window as unknown as Win).db.execute(query, params as never),
    { query, params },
  );

const rows = (d: Device, query: string, params: Record<string, unknown> = {}) =>
  d.page.evaluate(
    async ({ query, params }) =>
      (await (window as unknown as Win).db.query(query, params as never)).rows,
    { query, params },
  );

const status = (d: Device): Promise<SyncStatus> =>
  d.page.evaluate(() => (window as unknown as Win).db.sync.status());

const remote = (d: Device) =>
  d.page.evaluate(() => (window as unknown as Win).remote);

/**
 * The whole graph in canonical form. Entities are keyed by their global id
 * (`_gid`), which sync keeps identical on every device, so two devices hold
 * the same graph exactly when these strings are equal.
 */
function canonical(d: Device): Promise<string> {
  return d.page.evaluate(async () => {
    const db = (window as unknown as Win).db;
    const sorted = (v: unknown): unknown =>
      Array.isArray(v)
        ? v.map(sorted)
        : v !== null && typeof v === "object"
          ? Object.fromEntries(
              Object.entries(v as Record<string, unknown>)
                .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0))
                .map(([k, x]) => [k, sorted(x)]),
            )
          : v;
    const nodes = await db.query(
      "MATCH (n) RETURN n._gid, labels(n), properties(n)",
    );
    const edges = await db.query(
      "MATCH (a)-[r]->(b) RETURN r._gid, a._gid, b._gid, type(r), properties(r)",
    );
    const lines = [
      ...nodes.rows.map((r) =>
        JSON.stringify([
          "node",
          r[0],
          [...(r[1] as string[])].sort(),
          sorted(r[2]),
        ]),
      ),
      ...edges.rows.map((r) =>
        JSON.stringify(["edge", r[0], r[1], r[2], r[3], sorted(r[4])]),
      ),
    ];
    return lines.sort().join("\n");
  });
}

/** Poll until `check` returns true; on timeout fail with what `describe` returns. */
async function until(
  what: string,
  check: () => Promise<boolean>,
  describe: () => Promise<unknown>,
  timeoutMs = WAIT_MS,
): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    if (await check()) return;
    if (Date.now() > deadline)
      throw new Error(
        `timed out waiting for: ${what}\n${JSON.stringify(await describe(), null, 2)}`,
      );
    await new Promise((r) => setTimeout(r, 100));
  }
}

/**
 * Wait until every device holds the same graph, has nothing left to upload,
 * and `also` (a condition on that common graph) holds. Returns the graph.
 */
async function converged(
  devices: Device[],
  what: string,
  also: (graph: string) => boolean = () => true,
): Promise<string> {
  let graph = "";
  await until(
    what,
    async () => {
      const graphs = await Promise.all(devices.map(canonical));
      const states = await Promise.all(devices.map(status));
      graph = graphs[0]!;
      return (
        graphs.every((g) => g === graph) &&
        states.every((s) => s.pendingUploads === 0) &&
        also(graph)
      );
    },
    async () => ({
      graphs: await Promise.all(devices.map(canonical)),
      status: await Promise.all(devices.map(status)),
    }),
  );
  return graph;
}

/** Every file under `dir`, with its bytes. */
function filesUnder(dir: string): { file: string; bytes: Buffer }[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory()
      ? filesUnder(full)
      : [{ file: full, bytes: readFileSync(full) }];
  });
}

describe.skipIf(!relayBin)("sync between two browser contexts", () => {
  let a: Device;
  let b: Device;
  let code = "";

  it("B joins with A's pairing code and sees the data A already had", async () => {
    a = await newDevice("A");
    b = await newDevice("B");
    const openedA = await openDb(a, "sync-a");
    expect(openedA.status).toMatchObject({ enabled: false, running: false });

    // Written before sync exists on this database.
    await run(a, "CREATE (:Note {key: $k, text: $t})", {
      k: "n1",
      t: MARKER,
    });
    await run(a, "CREATE (:Note:Pinned {key: $k, text: $t, stars: $s})", {
      k: "n2",
      t: `it's "quoted" \\ 🐜`,
      s: 3,
    });
    await run(a, "CREATE (:Tag {key: $k})", { k: "t1" });
    await run(
      a,
      "MATCH (n:Note {key: 'n1'}), (t:Tag {key: 't1'}) CREATE (n)-[:TAGGED {weight: $w}]->(t)",
      { w: 0.5 },
    );

    code = await a.page.evaluate(
      (relayUrl) => (window as unknown as Win).db.sync.create({ relayUrl }),
      relayUrl,
    );
    expect(code).toMatch(/^gsp0-[A-Za-z0-9_-]{43}$/);
    expect(
      await a.page.evaluate(() =>
        (window as unknown as Win).db.sync.pairingCode(),
      ),
    ).toBe(code);

    await openDb(b, "sync-b");
    expect(await rows(b, "MATCH (n) RETURN count(n)")).toEqual([[0]]);
    await b.page.evaluate(
      ({ relayUrl, code }) =>
        (window as unknown as Win).db.sync.join({ relayUrl, code }),
      { relayUrl, code },
    );

    const graph = await converged([a, b], "B to receive A's data", (g) =>
      g.includes(MARKER),
    );
    expect(graph.split("\n")).toHaveLength(4); // 3 nodes + 1 edge
    expect(
      await rows(
        b,
        "MATCH (n:Note {key: 'n1'})-[r:TAGGED]->(t:Tag) RETURN n.text, r.weight, t.key",
      ),
    ).toEqual([[MARKER, 0.5, "t1"]]);
    expect(
      await rows(b, "MATCH (n:Pinned) RETURN n.text, n.stars, labels(n)"),
    ).toEqual([[`it's "quoted" \\ 🐜`, 3, ["Note", "Pinned"]]]);

    // B was told, so an app could re-run its queries.
    const told = await remote(b);
    expect(told.events).toBeGreaterThan(0);
    expect(told.changes).toBe(4);
    expect((await remote(a)).events).toBe(0);

    const [sa, sb] = [await status(a), await status(b)];
    for (const s of [sa, sb]) {
      expect(s).toMatchObject({
        enabled: true,
        running: true,
        connected: true,
        relayUrl,
        pendingUploads: 0,
        lastError: null,
        notUploaded: 0,
        notApplied: 0,
      });
    }
    await until(
      "both devices to know each other",
      async () =>
        (await status(a)).devices === 2 && (await status(b)).devices === 2,
      async () => [await status(a), await status(b)],
    );
  });

  it("concurrent edits on both sides converge to one state", async () => {
    // Fired together: neither side has seen the other's write when it writes.
    await Promise.all([
      run(a, "MATCH (n:Note {key: 'n2'}) SET n.text = 'edited on A'"),
      run(b, "MATCH (n:Note {key: 'n2'}) SET n.text = 'edited on B'"),
      run(a, "MATCH (n:Note {key: 'n1'}) SET n.owner = 'a'"),
      run(b, "MATCH (n:Note {key: 'n1'}) SET n:Shared, n.reviewer = 'b'"),
      run(a, "CREATE (:Note {key: 'from-a', text: 'new on A'})"),
      run(b, "CREATE (:Note {key: 'from-b', text: 'new on B'})"),
      run(
        b,
        "MATCH (n:Note {key: 'n2'}), (t:Tag {key: 't1'}) CREATE (n)-[:TAGGED {weight: 1}]->(t)",
      ),
      run(a, "MATCH (:Note)-[r:TAGGED]->(:Tag) SET r.checked = true"),
    ]);
    await converged([a, b], "concurrent edits to converge");

    for (const d of [a, b]) {
      // Same-property conflict: one of the two values, the same on both.
      const [[text]] = (await rows(
        d,
        "MATCH (n:Note {key: 'n2'}) RETURN n.text",
      )) as [[string]];
      expect(["edited on A", "edited on B"]).toContain(text);
      // Different properties and a label on one node: all kept.
      expect(
        await rows(
          d,
          "MATCH (n:Note:Shared {key: 'n1'}) RETURN n.owner, n.reviewer, n.text",
        ),
      ).toEqual([["a", "b", MARKER]]);
      expect(
        await rows(d, "MATCH (n:Note) RETURN n.key ORDER BY n.key"),
      ).toEqual([["from-a"], ["from-b"], ["n1"], ["n2"]]);
      expect(
        await rows(d, "MATCH (:Note)-[r:TAGGED]->(:Tag) RETURN count(r)"),
      ).toEqual([[2]]);
    }
    const textA = await rows(a, "MATCH (n:Note {key: 'n2'}) RETURN n.text");
    expect(await rows(b, "MATCH (n:Note {key: 'n2'}) RETURN n.text")).toEqual(
      textA,
    );
  });

  it("a delete propagates, with the edges of the deleted node", async () => {
    await run(b, "MATCH (n:Note {key: 'n2'}) DETACH DELETE n");
    await run(a, "MATCH (:Note {key: 'n1'})-[r:TAGGED]->() DELETE r");
    await converged(
      [a, b],
      "deletes to propagate",
      (g) => !g.includes('"n2"') && !g.includes("TAGGED"),
    );
    for (const d of [a, b]) {
      expect(
        await rows(d, "MATCH (n:Note) RETURN n.key ORDER BY n.key"),
      ).toEqual([["from-a"], ["from-b"], ["n1"]]);
      expect(await rows(d, "MATCH ()-[r]->() RETURN count(r)")).toEqual([[0]]);
      expect(await rows(d, "MATCH (t:Tag) RETURN count(t)")).toEqual([[1]]);
    }
  });

  it("writes made while B is offline sync once it is back", async () => {
    await b.ctx.setOffline(true);
    await run(b, "CREATE (:Note {key: 'offline-b', text: 'written offline'})");
    await run(b, "MATCH (n:Note {key: 'n1'}) SET n.offline = 'b'");

    // B notices it cannot reach the relay and keeps its writes queued.
    await until(
      "B to report it is disconnected with uploads queued",
      async () => {
        const s = await status(b);
        return !s.connected && s.pendingUploads >= 2 && s.lastError !== null;
      },
      () => status(b),
    );
    // B's own sync attempt fails instead of pretending. (It also ends the
    // long poll B had open from before: the browser's offline emulation
    // does not cut a request that is already in flight.)
    const failed = await b.page.evaluate(() =>
      (window as unknown as Win).db.sync.syncNow().then(
        () => "resolved",
        (e: Error & { code?: string }) => `${e.code}: ${e.message}`,
      ),
    );
    expect(failed).toMatch(/^sync: (offline|relay unreachable)$/);

    await run(a, "CREATE (:Note {key: 'while-b-away', text: 'A kept going'})");
    await run(a, "MATCH (n:Note {key: 'from-a'}) DETACH DELETE n");
    // A is up to date with the relay and has nothing of B's offline work.
    await a.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());
    expect(
      await rows(a, "MATCH (n:Note {key: 'offline-b'}) RETURN count(n)"),
    ).toEqual([[0]]);
    expect(
      await rows(b, "MATCH (n:Note {key: 'while-b-away'}) RETURN count(n)"),
    ).toEqual([[0]]);
    expect(await canonical(a)).not.toBe(await canonical(b));

    // Still nothing after a while: B is not quietly getting through.
    await new Promise((r) => setTimeout(r, 1_500));
    expect(
      await rows(b, "MATCH (n:Note {key: 'while-b-away'}) RETURN count(n)"),
    ).toEqual([[0]]);
    expect((await status(b)).pendingUploads).toBeGreaterThanOrEqual(2);

    await b.ctx.setOffline(false);
    await converged(
      [a, b],
      "both sides to converge after B came back",
      (g) => g.includes("offline-b") && g.includes("while-b-away"),
    );
    for (const d of [a, b]) {
      expect(
        await rows(d, "MATCH (n:Note) RETURN n.key ORDER BY n.key"),
      ).toEqual([["from-b"], ["n1"], ["offline-b"], ["while-b-away"]]);
      expect(
        await rows(d, "MATCH (n:Note {key: 'n1'}) RETURN n.offline"),
      ).toEqual([["b"]]);
    }
    expect((await status(b)).connected).toBe(true);
  });

  it("after a reload sync resumes by itself and nothing is duplicated", async () => {
    const before = await canonical(b);
    // No flush and no close: the page and its worker are simply killed.
    await b.page.reload();
    const reopened = await openDb(b, "sync-b");
    expect(reopened.restored).toBe(true);
    // No join this time: the group and the relay come from the stored state.
    expect(reopened.status).toMatchObject({
      enabled: true,
      running: true,
      relayUrl,
    });
    expect(
      await b.page.evaluate(() =>
        (window as unknown as Win).db.sync.pairingCode(),
      ),
    ).toBe(code);
    expect(await canonical(b)).toBe(before);

    await run(a, "CREATE (:Note {key: 'after-reload', text: 'from A'})");
    await run(b, "CREATE (:Note {key: 'b-after-reload', text: 'from B'})");
    await converged(
      [a, b],
      "sync to resume after B reloaded",
      (g) => g.includes("after-reload") && g.includes("b-after-reload"),
    );
    for (const d of [a, b]) {
      const keys = (await rows(d, "MATCH (n) RETURN n.key ORDER BY n.key")) as [
        string,
      ][];
      expect(keys).toEqual([
        ["after-reload"],
        ["b-after-reload"],
        ["from-b"],
        ["n1"],
        ["offline-b"],
        ["t1"],
        ["while-b-away"],
      ]);
      // Every global id once.
      const gids = (await rows(d, "MATCH (n) RETURN n._gid")) as [string][];
      expect(new Set(gids.map((g) => g[0])).size).toBe(gids.length);
    }
    // The reloaded device replayed nothing into the app that it already had:
    // only the one new node from A arrived as a remote change.
    expect((await remote(b)).changes).toBe(1);

    // A stopped device keeps its writes across a reload and uploads them on start().
    await b.page.evaluate(() => (window as unknown as Win).db.sync.stop());
    await run(b, "CREATE (:Note {key: 'while-stopped', text: 'queued'})");
    await b.page.evaluate(() => (window as unknown as Win).db.flush());
    await b.page.reload();
    const stopped = await openDb(b, "sync-b");
    expect(stopped.status).toMatchObject({ enabled: true, running: false });
    expect(stopped.status.pendingUploads).toBeGreaterThan(0);
    await a.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());
    expect(
      await rows(a, "MATCH (n:Note {key: 'while-stopped'}) RETURN count(n)"),
    ).toEqual([[0]]);
    await b.page.evaluate(() => (window as unknown as Win).db.sync.start());
    await converged([a, b], "the stopped device's write to arrive", (g) =>
      g.includes("while-stopped"),
    );
  });

  it("the relay's data directory holds no plaintext", async () => {
    // The marker really is in both databases...
    for (const d of [a, b])
      expect(await rows(d, "MATCH (n:Note {key: 'n1'}) RETURN n.text")).toEqual(
        [[MARKER]],
      );
    // ...and the relay really stored this group's items on disk.
    const files = filesUnder(relayDir);
    const logs = files.filter((f) => f.file.endsWith(".log"));
    expect(logs).toHaveLength(1);
    expect(statSync(logs[0]!.file).size).toBeGreaterThan(2_000);
    const needles = [
      MARKER,
      "written offline",
      "while-b-away",
      "TAGGED",
      "reviewer",
      // The pairing code and its secret part.
      code,
      code.slice(5),
    ];
    for (const { file, bytes } of files)
      for (const needle of needles)
        expect(
          bytes.includes(Buffer.from(needle)),
          `${needle} found in ${path.basename(file)}`,
        ).toBe(false);
    // Nor did the code reach any log: the relay's, the pages' or the workers'.
    for (const text of [relayLog, consoleLines.join("\n")]) {
      expect(text.includes(code.slice(5))).toBe(false);
      expect(text.includes(MARKER)).toBe(false);
    }
    expect(relayLog).toContain("event=");
  });

  it("a third device with a wrong pairing code sees nothing", async () => {
    const c = await newDevice("C");
    await openDb(c, "sync-c");
    // Well formed, but not this group's secret.
    const wrong = `gsp0-${randomBytes(32).toString("base64url")}`;
    expect(wrong).not.toBe(code);
    await c.page.evaluate(
      ({ relayUrl, wrong }) =>
        (window as unknown as Win).db.sync.join({ relayUrl, code: wrong }),
      { relayUrl, wrong },
    );
    // A full round trip with the relay succeeds and brings nothing.
    await c.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());
    const s = await status(c);
    expect(s).toMatchObject({ enabled: true, connected: true, devices: 1 });
    expect(await rows(c, "MATCH (n) RETURN count(n)")).toEqual([[0]]);
    expect((await remote(c)).events).toBe(0);
    // What C writes stays out of the real group, too.
    await run(c, "CREATE (:Note {key: 'intruder'})");
    await c.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());
    await a.page.evaluate(() => (window as unknown as Win).db.sync.syncNow());
    expect(
      await rows(a, "MATCH (n:Note {key: 'intruder'}) RETURN count(n)"),
    ).toEqual([[0]]);
    expect(await canonical(a)).toBe(await canonical(b));

    // A malformed code is refused, and the error does not repeat it.
    const d = await newDevice("D");
    await openDb(d, "sync-d");
    const malformed = `${code.slice(0, -3)}!!!`;
    const refused = await d.page.evaluate(
      ({ relayUrl, malformed }) =>
        (window as unknown as Win).db.sync
          .join({ relayUrl, code: malformed })
          .then(
            () => "joined",
            (e: Error & { code?: string }) => `${e.code}: ${e.message}`,
          ),
      { relayUrl, malformed },
    );
    expect(refused).toBe("sync: the pairing code is not valid");
    expect((await status(d)).enabled).toBe(false);
    await c.ctx.close();
    await d.ctx.close();
  });

  it("syncs over the IndexedDB fallback backend: join, converge, reload", async () => {
    const i = await newDevice("I");
    const workerUrl = "/test/e2e/site/worker-no-opfs.js";
    const opened = await openDb(i, "sync-idb", { workerUrl });
    expect(opened.backend).toBe("idb");
    await run(
      i,
      "CREATE (:Note {key: 'idb-before-join', text: 'local first'})",
    );
    await join(i, code);
    await converged(
      [a, b, i],
      "the IndexedDB device to join and converge",
      (g) => g.includes("idb-before-join"),
    );
    expect(await keys(i)).toEqual(await keys(a));
    expect(await rows(i, "MATCH (n:Note {key: 'n1'}) RETURN n.text")).toEqual([
      [MARKER],
    ]);

    // Reload without close: the state comes back from IndexedDB and sync resumes.
    const before = await canonical(i);
    await i.page.reload();
    const reopened = await openDb(i, "sync-idb", { workerUrl });
    expect(reopened).toMatchObject({ backend: "idb", restored: true });
    expect(reopened.status).toMatchObject({ enabled: true, running: true });
    expect(await canonical(i)).toBe(before);
    await run(a, "CREATE (:Note {key: 'for-idb', text: 'from A'})");
    await run(
      i,
      "MATCH (n:Note {key: 'idb-before-join'}) SET n.text = 'edited'",
    );
    await converged(
      [a, b, i],
      "sync to resume on the IndexedDB device",
      (g) => g.includes("for-idb") && g.includes("edited"),
    );
    const gids = (await rows(i, "MATCH (n) RETURN n._gid")) as [string][];
    expect(new Set(gids.map((g) => g[0])).size).toBe(gids.length);
    expect((await remote(i)).changes).toBe(1);
    await i.page.evaluate(() => (window as unknown as Win).db.close());
    await i.ctx.close();
  });

  it("a device with a wrong clock is reported and does not stall the others", async () => {
    // W's clock is three days ahead (in its worker, where sync stamps ops).
    const w = await newDevice("W");
    await openDb(w, "sync-w", {
      workerUrl: "/test/e2e/site/worker-clock-ahead.js",
    });
    await join(w, code);
    // W receives the group's data like any device.
    await until(
      "W to receive the group's data",
      async () => (await canonical(w)) === (await canonical(a)),
      async () => [await status(w), await canonical(w), await canonical(a)],
    );
    await run(w, "CREATE (:Note {key: 'from-the-future', text: 'W'})");
    await run(w, "MATCH (n:Note {key: 'n1'}) SET n.owner = 'w'");

    // The others refuse W's writes (they would win every conflict for three
    // days) and say whose clock is off, by how much.
    const DAY = 24 * 60 * 60 * 1000;
    for (const d of [a, b]) {
      await until(
        `${d.label} to report the device whose clock is ahead`,
        async () =>
          (await status(d)).deviceList.some((x) => x.clockLeadMs > DAY),
        () => status(d),
      );
      const s = await status(d);
      const ahead = s.deviceList.filter((x) => x.clockLeadMs > 0);
      expect(ahead).toHaveLength(1);
      expect(ahead[0]).toMatchObject({ own: false, appliedSeq: 0 });
      expect(ahead[0]!.clockLeadMs).toBeGreaterThan(2.9 * DAY);
      expect(ahead[0]!.clockLeadMs).toBeLessThan(3.1 * DAY);
      expect(s.deviceList.filter((x) => x.own)).toHaveLength(1);
      expect(s.notApplied).toBeGreaterThan(0);
      expect(s.needsRejoin).toBe(false);
      expect(await keys(d)).not.toContain("from-the-future");
      expect(
        await rows(d, "MATCH (n:Note {key: 'n1'}) RETURN n.owner"),
      ).toEqual([["a"]]);
    }

    // Their own writes, and each other's, still flow; W receives them too.
    await run(a, "CREATE (:Note {key: 'a-despite-w', text: 'A'})");
    await run(b, "CREATE (:Note {key: 'b-despite-w', text: 'B'})");
    await run(b, "MATCH (n:Note {key: 'a-despite-w'}) SET n.seen = true");
    await converged(
      [a, b],
      "A and B to keep converging next to the wrong-clock device",
      (g) => g.includes("a-despite-w") && g.includes("b-despite-w"),
    );
    await until(
      "W to receive what A and B wrote",
      async () => {
        const k = await keys(w);
        return k.includes("a-despite-w") && k.includes("b-despite-w");
      },
      async () => [await status(w), await keys(w)],
    );
    expect((await status(w)).pendingUploads).toBe(0);
    await w.page.evaluate(() => (window as unknown as Win).db.close());
    await w.ctx.close();
  });

  /**
   * A device in its own browser profile directory, so the whole of its
   * storage (snapshot, keys, rollback counter) can be put back as it was.
   */
  function profileDevice(label: string, dbName: string) {
    const profile = path.join(scratch, `profile-${label}`);
    const backup = `${profile}.backup`;
    return {
      async launch() {
        const ctx = await chromium.launchPersistentContext(profile);
        const d = await newDevice(label, ctx);
        const opened = await openDb(d, dbName);
        return { d, opened };
      },
      async shut(d: Device) {
        await d.page.evaluate(() => (window as unknown as Win).db.close());
        await d.ctx.close();
      },
      /** Copy the (closed) profile aside. */
      backUp: () => cpSync(profile, backup, { recursive: true }),
      /** Put the copy back over the (closed) profile. */
      restore() {
        rmSync(profile, { recursive: true, force: true });
        cpSync(backup, profile, { recursive: true });
      },
    };
  }

  const oncePerKey = async (d: Device, wanted: string[]) => {
    const k = await keys(d);
    for (const key of wanted)
      expect(
        k.filter((x) => x === key),
        `${key} on ${d.label}`,
      ).toHaveLength(1);
  };

  it("a device restored from an older copy catches up by itself when it can read the relay first", async () => {
    const p = profileDevice("G", "sync-g");
    let { d: g } = await p.launch();
    await join(g, code);
    await run(g, "CREATE (:Note {key: 'g-base', text: 'before the backup'})");
    await converged([a, g], "G to join", (x) => x.includes("g-base"));
    const devicesBefore = (await status(a)).devices;
    await p.shut(g);
    p.backUp();

    // G goes on and uploads writes that the backup does not have...
    ({ d: g } = await p.launch());
    await run(
      g,
      "CREATE (:Note {key: 'g-w1', text: 'uploaded, then lost locally'})",
    );
    await run(
      g,
      "MATCH (n:Note {key: 'g-base'}) SET n.text = 'edited before the restore'",
    );
    await converged([a, g], "G's writes to reach A", (x) => x.includes("g-w1"));
    await p.shut(g);
    // ...and then its storage is rolled back to the backup.
    p.restore();

    let opened;
    ({ d: g, opened } = await p.launch());
    expect(opened.restored).toBe(true);
    expect(opened.status).toMatchObject({ enabled: true, running: true });
    // The app writes at once, without waiting for anything: the write is in
    // the database immediately and is held from upload until G has read its
    // own earlier uploads back from the relay.
    await run(
      g,
      "CREATE (:Note {key: 'g-during', text: 'written right after opening'})",
    );
    expect(await keys(g)).toContain("g-during");

    await converged(
      [a, b, g],
      "the restored device to catch up and upload",
      (x) => x.includes("g-during") && x.includes("edited before the restore"),
    );
    const s = await status(g);
    expect(s).toMatchObject({
      needsRejoin: false,
      catchingUp: false,
      connected: true,
      pendingUploads: 0,
      notUploaded: 0,
    });
    // Nothing lost, nothing twice, on any device.
    for (const d of [a, b, g])
      await oncePerKey(d, ["g-base", "g-w1", "g-during"]);
    // It got its own lost writes back as remote changes.
    expect((await remote(g)).changes).toBeGreaterThanOrEqual(2);
    // To the others it is the same device as before, in good standing.
    const sa = await status(a);
    expect(sa.devices).toBe(devicesBefore);
    expect(sa.deviceList.every((x) => !x.equivocated)).toBe(true);
    // And its next write arrives like any other.
    await run(g, "CREATE (:Note {key: 'g-after', text: 'G'})");
    await converged([a, b, g], "G to sync normally again", (x) =>
      x.includes("g-after"),
    );
    expect(await failure(g, "rejoinAsNewDevice")).toMatch(
      /^sync: this device does not need to rejoin/,
    );
    await p.shut(g);
  });

  it("a restored device that uploads before reading the relay needs to rejoin, and loses none of its writes", async () => {
    // The one way to write before reading: sync is stopped, so nothing reads
    // the relay and writes are queued for upload as usual. The backup is
    // taken in that state, which makes the order of events certain.
    const p = profileDevice("F", "sync-f");
    let { d: f } = await p.launch();
    await join(f, code);
    await run(f, "CREATE (:Note {key: 'f-base', text: 'before the backup'})");
    await converged([a, f], "F to join", (x) => x.includes("f-base"));
    const devicesBefore = (await status(a)).devices;
    await f.page.evaluate(() => (window as unknown as Win).db.sync.stop());
    await p.shut(f);
    p.backUp();

    // F goes on, uploads a write...
    ({ d: f } = await p.launch());
    await f.page.evaluate(() => (window as unknown as Win).db.sync.start());
    await run(
      f,
      "CREATE (:Note {key: 'f-w1', text: 'uploaded, then lost locally'})",
    );
    await converged([a, f], "F's write to reach A", (x) => x.includes("f-w1"));
    await p.shut(f);
    // ...and then its storage is rolled back to the backup.
    p.restore();

    let opened;
    ({ d: f, opened } = await p.launch());
    expect(opened.restored).toBe(true);
    expect(opened.status).toMatchObject({
      enabled: true,
      running: false,
      needsRejoin: false,
      catchingUp: false,
    });
    // Still stopped, it writes: queued for upload under a position in its
    // history that it has already used for `f-w1`.
    await run(
      f,
      "CREATE (:Note {key: 'f-w2', text: 'written before reading the relay'})",
    );
    expect((await status(f)).pendingUploads).toBe(1);
    expect(await keys(f)).not.toContain("f-w1");

    // On start it finds its own lost write on the relay: a conflict.
    await f.page.evaluate(() => (window as unknown as Win).db.sync.start());
    await until(
      "F to notice the conflict with what it had uploaded",
      async () => (await status(f)).needsRejoin,
      () => status(f),
    );
    expect(await keys(f)).toContain("f-w1"); // it follows what it had published

    // Writes still work locally, but go nowhere.
    await run(f, "CREATE (:Note {key: 'f-w3', text: 'written while forked'})");
    await run(
      f,
      "MATCH (n:Note {key: 'f-base'}) SET n.text = 'edited while forked'",
    );
    await run(a, "CREATE (:Note {key: 'a-meanwhile', text: 'A'})");
    await until(
      "F to keep receiving while forked",
      async () => (await keys(f)).includes("a-meanwhile"),
      () => status(f),
    );
    await syncNow(a);
    const onA = await keys(a);
    expect(onA).not.toContain("f-w2");
    expect(onA).not.toContain("f-w3");
    expect(await status(f)).toMatchObject({ needsRejoin: true, running: true });
    // ...also across a reload.
    await p.shut(f);
    ({ d: f, opened } = await p.launch());
    expect(opened.status.needsRejoin).toBe(true);
    await oncePerKey(f, ["f-w2", "f-w3"]);

    // Only the forked device can rejoin.
    expect(await failure(a, "rejoinAsNewDevice")).toMatch(
      /^sync: this device does not need to rejoin/,
    );
    await f.page.evaluate(() =>
      (window as unknown as Win).db.sync.rejoinAsNewDevice(),
    );
    expect((await status(f)).needsRejoin).toBe(false);
    await converged(
      [a, b, f],
      "the rejoined device's writes to arrive",
      (x) =>
        x.includes("f-w2") &&
        x.includes("f-w3") &&
        x.includes("edited while forked"),
    );
    for (const d of [a, b, f])
      await oncePerKey(d, ["f-base", "f-w1", "f-w2", "f-w3", "a-meanwhile"]);
    // To the others it is one device more, and the old one is marked.
    await until(
      "A to see the new identity",
      async () => (await status(a)).devices === devicesBefore + 1,
      () => status(a),
    );
    expect(
      (await status(a)).deviceList.filter((x) => x.equivocated),
    ).toHaveLength(1);
    // And it can write again.
    await run(f, "CREATE (:Note {key: 'f-after-rejoin', text: 'F'})");
    await converged([a, b, f], "F to sync normally again", (x) =>
      x.includes("f-after-rejoin"),
    );
    await p.shut(f);
  });

  it("a relay that requires a registration secret: create needs it, join does not", async () => {
    const SECRET = "registration-secret-quokka-5512";
    const dir = path.join(scratch, "relay-registration");
    mkdirSync(dir);
    const { child, url } = await startRelay(dir, 0, SECRET);
    const r1 = await newDevice("R1");
    await openDb(r1, "sync-r1");
    await run(r1, "CREATE (:Note {key: 'r1', text: 'registered'})");

    // Without the secret: a clear error, and no half-made group.
    expect(await failure(r1, "create", { relayUrl: url })).toBe(
      "sync: this relay requires a registration token to create a sync group; pass registrationToken to sync.create()",
    );
    expect(await status(r1)).toMatchObject({ enabled: false, running: false });
    // With a wrong one: also refused.
    expect(
      await failure(r1, "create", {
        relayUrl: url,
        registrationToken: "wrong",
      }),
    ).toBe(
      "sync: the relay did not accept the registration token; no sync group was created",
    );
    expect((await status(r1)).enabled).toBe(false);
    expect(filesUnder(dir).filter((f) => f.file.endsWith(".log"))).toEqual([]);

    // With it: created and registered.
    const groupCode = await r1.page.evaluate(
      ({ url, SECRET }) =>
        (window as unknown as Win).db.sync.create({
          relayUrl: url,
          registrationToken: SECRET,
        }),
      { url, SECRET },
    );
    expect(groupCode).toMatch(/^gsp0-/);
    expect(filesUnder(dir).filter((f) => f.file.endsWith(".log"))).toHaveLength(
      1,
    );

    // A second device joins with the pairing code alone.
    const r2 = await newDevice("R2");
    await openDb(r2, "sync-r2");
    await join(r2, groupCode, url);
    await run(r2, "CREATE (:Note {key: 'r2', text: 'joined'})");
    await converged(
      [r1, r2],
      "the two devices on the closed relay",
      (g) => g.includes("registered") && g.includes("joined"),
    );
    for (const d of [r1, r2])
      expect(await status(d)).toMatchObject({
        connected: true,
        lastError: null,
      });

    // The secret is in no log and not on the relay's disk.
    for (const text of [relayLog, consoleLines.join("\n")])
      expect(text.includes(SECRET)).toBe(false);
    for (const { bytes } of filesUnder(dir))
      expect(bytes.includes(Buffer.from(SECRET))).toBe(false);
    // Nor in what the device stores: after a reload it is simply gone.
    await r1.page.reload();
    await openDb(r1, "sync-r1");
    await run(r1, "CREATE (:Note {key: 'r1-later', text: 'no secret needed'})");
    await converged([r1, r2], "sync after reload without the secret", (g) =>
      g.includes("r1-later"),
    );
    for (const d of [r1, r2]) {
      await d.page.evaluate(() => (window as unknown as Win).db.close());
      await d.ctx.close();
    }
    await stopRelay(child);
  });

  it("only https relays, except on this machine", async () => {
    const d = await newDevice("H");
    await openDb(d, "sync-h");
    expect(
      await failure(d, "create", { relayUrl: "http://relay.example.org" }),
    ).toBe(
      "sync: relayUrl must use https:// (plain http:// is accepted for localhost, 127.0.0.1 and [::1] only)",
    );
    expect(
      await failure(d, "join", { relayUrl: "ftp://relay.example.org", code }),
    ).toBe("sync: relayUrl must be an https:// URL");
    expect(
      await failure(d, "create", {
        relayUrl: "https://relay.example.org/?a=1",
      }),
    ).toMatch(/must not carry credentials, a query or a fragment/);
    expect((await status(d)).enabled).toBe(false);
    await d.ctx.close();
  });

  it("a relay that lost its data: devices notice, keep syncing, and report that history is gone", async () => {
    await converged([a, b], "A and B to be level before the relay is wiped");
    const before = await canonical(a);
    expect((await status(a)).relayResets).toBe(0);

    // The relay comes back on the same address with an empty data directory.
    const port = Number(new URL(relayUrl).port);
    await stopRelay(mainRelay);
    rmSync(relayDir, { recursive: true, force: true });
    mkdirSync(relayDir);
    mainRelay = (await startRelay(relayDir, port)).child;
    expect(filesUnder(relayDir).filter((f) => f.file.endsWith(".log"))).toEqual(
      [],
    );

    // Both devices find out by themselves and lose nothing they had.
    for (const d of [a, b]) {
      await until(
        `${d.label} to notice the relay was reset`,
        async () => {
          const s = await status(d);
          return s.relayResets === 1 && s.connected;
        },
        () => status(d),
      );
      expect(await canonical(d)).toBe(before);
    }

    // They go on syncing with each other through the new, empty log.
    await run(a, "CREATE (:Note {key: 'a-after-reset', text: 'A'})");
    await run(b, "CREATE (:Note {key: 'b-after-reset', text: 'B'})");
    await run(b, "MATCH (n:Note {key: 'n1'}) SET n.owner = 'b after reset'");
    await converged(
      [a, b],
      "A and B to converge through the wiped relay",
      (g) => g.includes("a-after-reset") && g.includes("b-after-reset"),
    );
    expect(await rows(a, "MATCH (n:Note {key: 'n1'}) RETURN n.owner")).toEqual([
      ["b after reset"],
    ]);
    for (const d of [a, b])
      expect(await status(d)).toMatchObject({
        relayResets: 1,
        needsRejoin: false,
        lastError: null,
        pendingUploads: 0,
      });

    // What only the relay held is gone: a device that joins now cannot read
    // what A and B write (it never gets their first messages), and says so.
    const n = await newDevice("N");
    await openDb(n, "sync-n");
    await join(n, code);
    await run(n, "CREATE (:Note {key: 'n-new-device', text: 'N'})");
    await syncNow(n);
    await until(
      "the late device to hold A's and B's commits as unusable",
      async () => (await status(n)).pendingDownloads >= 3,
      () => status(n),
    );
    expect(await keys(n)).toEqual(["n-new-device"]);
    expect((await remote(n)).events).toBe(0);
    expect((await status(n)).deviceList).toHaveLength(1);
    // The others do receive what the late device writes.
    await converged([a, b], "A and B to receive the late device's write", (g) =>
      g.includes("n-new-device"),
    );
    await n.page.evaluate(() => (window as unknown as Win).db.close());
    await n.ctx.close();
  });

  it("close() stops syncing cleanly", async () => {
    for (const d of [a, b]) {
      await d.page.evaluate(() => (window as unknown as Win).db.close());
      await d.ctx.close();
    }
    // Nothing printed an error on the way.
    expect(consoleLines.filter((l) => /pageerror/.test(l))).toEqual([]);
  });
});
