import { describe, expect, it } from "vitest";
import { SiafuError } from "../../src/errors.js";
import {
  normalizeRelayUrl,
  SyncLoop,
  type SyncEngine,
  type SyncHost,
} from "../../src/sync.js";

const TOKEN = "TOKEN-THAT-MUST-STAY-IN-THE-HEADER";
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function until(check: () => boolean, ms = 2_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (!check()) {
    if (Date.now() > deadline) throw new Error("timed out");
    await sleep(2);
  }
}

/** An engine stand-in: an outbox of numbered items, pushed three per frame. */
class FakeEngine implements SyncEngine {
  outbox: number[] = [];
  inFlight = 0;
  cursor = 0;
  ingested: number[] = [];
  syncPushFrame() {
    const part = this.outbox.slice(0, 3);
    this.inFlight = part.length;
    return part.length ? new Uint8Array(part) : undefined;
  }
  syncAcceptAck(reply: Uint8Array) {
    const n = Math.min(reply[0]!, this.inFlight);
    this.outbox.splice(0, n);
    this.inFlight = 0;
    return n;
  }
  syncPullFrame() {
    return new Uint8Array([this.cursor]);
  }
  syncIngestBatch(reply: Uint8Array) {
    const [more, ...items] = reply;
    this.ingested.push(...items);
    this.cursor += items.length;
    return JSON.stringify({
      items: items.length,
      ops: items.length,
      changes: items.length,
      more: more === 1,
      stalled: false,
    });
  }
  syncPoll() {
    return 0;
  }
  syncRelayToken() {
    return TOKEN;
  }
}

/** A relay stand-in behind `fetch`: a log of items, two per batch, long polls held. */
class FakeRelay {
  log: number[] = [];
  down = false;
  status = 200;
  requests: { path: string; wait: number; auth: string | null }[] = [];
  active = { push: 0, pull: 0 };
  maxActive = { push: 0, pull: 0 };
  private held: (() => void)[] = [];

  release() {
    for (const h of this.held.splice(0)) h();
  }

  fetch = async (url: string, init: RequestInit): Promise<Response> => {
    const u = new URL(url);
    const kind = u.pathname.endsWith("/push") ? "push" : "pull";
    const wait = Number(u.searchParams.get("wait_ms") ?? 0);
    this.requests.push({
      path: u.pathname,
      wait,
      auth: new Headers(init.headers).get("authorization"),
    });
    this.active[kind] += 1;
    this.maxActive[kind] = Math.max(this.maxActive[kind], this.active[kind]);
    try {
      await sleep(1);
      if (init.signal?.aborted) throw new DOMException("aborted", "AbortError");
      if (this.down) throw new TypeError(`fetch failed for ${TOKEN}`);
      if (this.status !== 200)
        return new Response("no", { status: this.status });
      const body = new Uint8Array(init.body as Uint8Array);
      if (kind === "push") {
        this.log.push(...body);
        this.release();
        return new Response(new Uint8Array([body.length]));
      }
      const after = body[0]!;
      if (wait > 0 && this.log.length <= after)
        await new Promise<void>((resolve, reject) => {
          const aborted = () =>
            reject(new DOMException("aborted", "AbortError"));
          if (init.signal?.aborted) return aborted();
          this.held.push(resolve);
          init.signal?.addEventListener("abort", aborted);
        });
      const items = this.log.slice(after, after + 2);
      const more = after + items.length < this.log.length ? 1 : 0;
      return new Response(new Uint8Array([more, ...items]));
    } finally {
      this.active[kind] -= 1;
    }
  };
}

function setup(options: { online?: boolean } = {}) {
  const engine = new FakeEngine();
  const relay = new FakeRelay();
  const seen = {
    disk: "ok" as "ok" | "full",
    /** Number of requests made so far, at each call of `durable`. */
    durable: [] as number[],
    persisted: 0,
    touched: 0,
    applied: 0,
    online: options.online ?? true,
  };
  const onlineListeners = new Set<() => void>();
  const host: SyncHost = {
    fetch: relay.fetch,
    now: () => Date.now(),
    random: () => 0.5,
    isOnline: () => seen.online,
    onOnline: (l) => {
      onlineListeners.add(l);
      return () => onlineListeners.delete(l);
    },
    persist: async () => {
      seen.persisted += 1;
    },
    durable: async () => {
      if (seen.disk === "full") throw new Error("QuotaExceededError");
      seen.durable.push(relay.requests.length);
    },
    touched: () => {
      seen.touched += 1;
    },
    applied: (n) => {
      seen.applied += n;
    },
  };
  const loop = new SyncLoop(engine, host, {
    relayUrl: "http://relay.test/base/",
    longPollMs: 5_000,
    pushDebounceMs: 5,
    minBackoffMs: 4,
    maxBackoffMs: 40,
  });
  const goOnline = () => {
    seen.online = true;
    for (const l of [...onlineListeners]) l();
  };
  return { engine, relay, host, loop, seen, goOnline, onlineListeners };
}

describe("normalizeRelayUrl", () => {
  it("accepts http(s) URLs and strips trailing slashes", () => {
    expect(normalizeRelayUrl("https://relay.example.org/")).toBe(
      "https://relay.example.org",
    );
    expect(normalizeRelayUrl("http://127.0.0.1:8787")).toBe(
      "http://127.0.0.1:8787",
    );
    expect(normalizeRelayUrl("https://example.org/gsp//")).toBe(
      "https://example.org/gsp",
    );
  });
  it("refuses anything else", () => {
    for (const bad of [
      "",
      "relay.example.org",
      "ftp://relay.example.org",
      "javascript:alert(1)",
      "https://user:pw@relay.example.org",
      "https://relay.example.org/?token=1",
      "https://relay.example.org/#x",
    ])
      expect(() => normalizeRelayUrl(bad), bad).toThrow(SiafuError);
  });
});

describe("SyncLoop", () => {
  it("pushes the outbox after a write, pulls what others pushed, and never overlaps itself", async () => {
    const { engine, relay, loop, seen } = setup();
    relay.log.push(101, 102, 103, 104, 105); // already on the relay
    engine.outbox.push(1, 2, 3, 4, 5, 6, 7);
    loop.start();
    loop.start(); // idempotent
    await until(() => engine.outbox.length === 0 && engine.cursor === 12);
    expect(relay.log.slice(5).sort()).toEqual([1, 2, 3, 4, 5, 6, 7]);
    expect(engine.ingested.slice(0, 5)).toEqual([101, 102, 103, 104, 105]);
    expect(seen.applied).toBe(12);
    expect(seen.persisted).toBeGreaterThanOrEqual(3);
    expect(seen.touched).toBe(3); // 7 items, 3 per frame

    // A burst of writes becomes one push after the debounce.
    const pushesBefore = relay.requests.filter((r) =>
      r.path.endsWith("/push"),
    ).length;
    for (const n of [8, 9]) {
      engine.outbox.push(n);
      loop.notifyLocalWrite();
    }
    await until(() => engine.outbox.length === 0 && engine.cursor === 14);
    expect(relay.requests.filter((r) => r.path.endsWith("/push")).length).toBe(
      pushesBefore + 1,
    );

    // Catch-up pulls do not wait; the idle one is a long poll.
    await until(() => relay.requests.at(-1)!.wait === 5_000);
    expect(relay.requests[0]!.path).toMatch(/^\/base\/v0\/(push|pull)$/);
    expect(relay.requests.every((r) => r.auth === `Bearer ${TOKEN}`)).toBe(
      true,
    );
    expect(relay.maxActive).toEqual({ push: 1, pull: 1 });
    expect(loop.state()).toMatchObject({
      running: true,
      connected: true,
      lastError: null,
    });

    await loop.stop();
    expect(loop.state()).toMatchObject({ running: false, connected: false });
    expect(relay.active).toEqual({ push: 0, pull: 0 }); // the long poll was aborted
    const total = relay.requests.length;
    engine.outbox.push(10);
    loop.notifyLocalWrite();
    await sleep(30);
    expect(relay.requests.length).toBe(total);
  });

  it("backs off while the relay is unreachable and recovers, keeping secrets out of errors", async () => {
    const { engine, relay, loop } = setup();
    relay.down = true;
    engine.outbox.push(1, 2);
    loop.start();
    await until(() => relay.requests.length >= 6);
    const state = loop.state();
    expect(state).toMatchObject({
      running: true,
      connected: false,
      lastError: "relay unreachable",
    });
    expect(engine.outbox).toEqual([1, 2]); // nothing lost
    const failure = await loop.syncNow().then(
      () => null,
      (e: Error) => e,
    );
    expect(failure).toBeInstanceOf(SiafuError);
    expect((failure as SiafuError).code).toBe("sync");
    expect(JSON.stringify([failure!.message, loop.state()])).not.toContain(
      TOKEN,
    );
    // Retries are spaced out: far fewer than one per millisecond.
    const before = relay.requests.length;
    await sleep(100);
    expect(relay.requests.length - before).toBeLessThan(16);

    relay.down = false;
    await until(() => engine.outbox.length === 0 && loop.state().connected);
    expect(relay.log).toEqual([1, 2]);
    expect(loop.state().lastError).toBeNull();
    expect(relay.maxActive).toEqual({ push: 1, pull: 1 });
    await loop.stop();
  });

  it("saves before every push and sends nothing if it cannot save", async () => {
    const { engine, relay, loop, seen } = setup();
    seen.disk = "full";
    engine.outbox.push(1, 2, 3, 4);
    loop.start();
    await until(() => loop.state().lastError !== null);
    expect(loop.state().lastError).toBe(
      "could not save before uploading; nothing was sent",
    );
    await sleep(30);
    expect(relay.requests.filter((r) => r.path.endsWith("/push"))).toEqual([]);
    expect(engine.outbox).toEqual([1, 2, 3, 4]);

    seen.disk = "ok";
    await until(() => engine.outbox.length === 0);
    // Each of the two push requests was preceded by its own save.
    const pushes = relay.requests
      .map((r, i) => (r.path.endsWith("/push") ? i : -1))
      .filter((i) => i >= 0);
    expect(pushes).toHaveLength(2);
    for (const [n, at] of pushes.entries())
      expect(seen.durable.filter((d) => d <= at).length).toBeGreaterThan(n);
    await loop.stop();
  });

  it("treats a refusal by the relay as an error with a long delay", async () => {
    const { engine, relay, loop } = setup();
    relay.status = 401;
    engine.outbox.push(1);
    loop.start();
    await until(() => loop.state().lastError !== null);
    expect(loop.state().lastError).toBe("relay answered 401");
    await sleep(60);
    // Two loops, at most the first try plus a few retries at the 20-40 ms ceiling.
    expect(relay.requests.length).toBeLessThanOrEqual(8);
    expect(engine.outbox).toEqual([1]);
    await loop.stop();
  });

  it("makes no request while offline and resumes on the online event", async () => {
    const { engine, relay, loop, goOnline, onlineListeners } = setup({
      online: false,
    });
    engine.outbox.push(1);
    loop.start();
    await sleep(40);
    expect(relay.requests).toEqual([]);
    expect(loop.state()).toMatchObject({
      connected: false,
      lastError: "offline",
    });
    goOnline();
    await until(() => engine.outbox.length === 0 && loop.state().connected);
    await loop.stop();
    expect(onlineListeners.size).toBe(0);
  });

  it("syncNow turns an idle long poll into an immediate round trip", async () => {
    const { engine, relay, loop } = setup();
    loop.start();
    await until(() => relay.requests.at(-1)?.wait === 5_000);
    // Something arrives on the relay without waking the long poll.
    relay.log.push(7, 8, 9);
    engine.outbox.push(1);
    await loop.syncNow();
    expect(engine.ingested).toEqual([7, 8, 9, 1]);
    expect(engine.outbox).toEqual([]);
    expect(relay.maxActive).toEqual({ push: 1, pull: 1 });
    await loop.stop();
    await expect(loop.syncNow()).rejects.toThrow("sync was stopped");
  });

  it("stop() during a request ends cleanly and rejects waiting syncNow calls", async () => {
    const { relay, loop } = setup();
    loop.start();
    await until(() => relay.requests.at(-1)?.wait === 5_000);
    relay.down = true;
    const pending = loop.syncNow().then(
      () => "resolved",
      (e: Error) => e.message,
    );
    await loop.stop();
    expect(["sync was stopped", "relay unreachable"]).toContain(await pending);
    expect(relay.active).toEqual({ push: 0, pull: 0 });
  });
});
