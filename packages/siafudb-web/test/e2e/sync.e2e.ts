/**
 * Device-to-device sync, end to end: two browser contexts (separate storage),
 * each with its own database, converge through a real `gsp-relay` process
 * that stores only ciphertext.
 *
 * The relay binary is not part of this repository. Build it from the
 * protocol repository (`cargo build -p gsp-relay --release`) and point
 * `GSP_RELAY_BIN` at it. Without the variable the usual local build
 * locations are tried; if none exists the suite is skipped with a warning.
 * With the variable set, a missing binary is a failure.
 */
import { spawn, type ChildProcess } from "node:child_process";
import { randomBytes } from "node:crypto";
import {
  createReadStream,
  existsSync,
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
if (!relayBin)
  console.warn(
    "sync.e2e: SKIPPED, no gsp-relay binary found. Build it from the protocol repository and set GSP_RELAY_BIN.",
  );

let server: http.Server;
let origin = "";
let browser: Browser;
let relay: ChildProcess | undefined;
let relayUrl = "";
let relayDir = "";
let relayLog = "";
/** Everything the pages and their workers printed. */
const consoleLines: string[] = [];

/** Start the relay on a free port; resolves to its base URL. */
function startRelay(bin: string, dataDir: string): Promise<string> {
  return new Promise((resolve, reject) => {
    const child = spawn(
      bin,
      ["--addr", "127.0.0.1:0", "--data-dir", dataDir],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
    relay = child;
    const timer = setTimeout(
      () => reject(new Error("gsp-relay did not start within 20 s")),
      20_000,
    );
    child.stderr!.setEncoding("utf8");
    child.stderr!.on("data", (chunk: string) => {
      relayLog += chunk;
      const m = /listening on (http:\/\/127\.0\.0\.1:\d+)/.exec(relayLog);
      if (m) {
        clearTimeout(timer);
        resolve(m[1]!);
      }
    });
    child.once("error", (e) => {
      clearTimeout(timer);
      reject(e);
    });
    child.once("exit", (code) => {
      clearTimeout(timer);
      reject(new Error(`gsp-relay exited early with code ${code}`));
    });
  });
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
  relayDir = mkdtempSync(path.join(tmpdir(), "siafudb-relay-"));
  relayUrl = await startRelay(relayBin, relayDir);
  browser = await chromium.launch();
});

afterAll(async () => {
  await browser?.close();
  server?.close();
  if (relay && relay.exitCode === null) {
    const gone = new Promise((r) => relay!.once("exit", r));
    relay.kill("SIGTERM");
    await gone;
  }
  if (relayDir) rmSync(relayDir, { recursive: true, force: true });
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

async function newDevice(label: string): Promise<Device> {
  const ctx = await browser.newContext();
  const page = await ctx.newPage();
  const note = (text: string) => consoleLines.push(`[${label}] ${text}`);
  page.on("pageerror", (e) => note(`pageerror ${e.message}`));
  page.on("console", (m) => note(m.text()));
  page.on("worker", (w) => w.on("console", (m) => note(m.text())));
  await page.goto(`${origin}/test/e2e/site/index.html`);
  return { label, ctx, page };
}

/** Open the database `name` as `window.db` and count remote-change events. */
async function openDb(d: Device, name: string) {
  return d.page.evaluate(async (name) => {
    const w = window as unknown as Win;
    const { SiafuDB } = await w.siafuReady;
    const db = await SiafuDB.open(name, { autosaveMs: 50 });
    w.db = db;
    w.remote = { events: 0, changes: 0 };
    db.sync.onRemoteChange((e) => {
      w.remote.events += 1;
      w.remote.changes += e.changes;
    });
    return { restored: db.restored, status: await db.sync.status() };
  }, name);
}

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
      const keys = (await rows(
        d,
        "MATCH (n) RETURN n.key ORDER BY n.key",
      )) as [string][];
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
      expect(
        await rows(d, "MATCH (n:Note {key: 'n1'}) RETURN n.text"),
      ).toEqual([[MARKER]]);
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

  it("close() stops syncing cleanly", async () => {
    for (const d of [a, b]) {
      await d.page.evaluate(() => (window as unknown as Win).db.close());
      await d.ctx.close();
    }
    // Nothing printed an error on the way.
    expect(consoleLines.filter((l) => /pageerror/.test(l))).toEqual([]);
  });
});
