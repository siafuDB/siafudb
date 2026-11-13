import { describe, expect, it } from "vitest";
import { SiafuError } from "../../src/errors.js";
import {
  parseRelayUrl,
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
    this.unechoed.push(...this.outbox.splice(0, n));
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
    this.unechoed = this.unechoed.filter((n) => !items.includes(n));
    if (this.stuck && items.length > 0) this.needsResync = true;
    // A relay that holds less than was read from it has lost its data.
    const relayReset = more === 2;
    if (relayReset) {
      this.cursor = 0;
      this.outbox.unshift(...this.unechoed);
      this.unechoed = [];
    }
    // At the relay's head, writes that were held are queued.
    const queued = more === 1 ? 0 : this.held.length;
    if (more !== 1) this.outbox.push(...this.held.splice(0));
    return JSON.stringify({
      items: items.length,
      ops: items.length,
      changes: items.length,
      more: more === 1,
      relayReset,
      queued,
      needsResync: this.needsResync,
      forked: false,
    });
  }
  syncPoll() {
    return 0;
  }
  origins: string[] = [];
  syncRelayToken(origin: string) {
    this.origins.push(origin);
    return TOKEN;
  }
  needsResync = false;
  resyncs: number[] = [];
  /** Acknowledged items the relay has not handed back. */
  unechoed: number[] = [];
  syncStartResync() {
    this.resyncs.push(Date.now());
    this.cursor = 0;
    this.needsResync = false;
  }
  /** Local writes held until the relay has been read (as after opening). */
  held: number[] = [];
  releases = 0;
  syncReleaseWrites() {
    this.releases += 1;
    const n = this.held.length;
    this.outbox.push(...this.held.splice(0));
    return n;
  }
  /** A device whose ops can never apply: every replay raises the flag again. */
  stuck = false;
}

/** A relay stand-in behind `fetch`: a log of items, two per batch, long polls held. */
class FakeRelay {
  log: number[] = [];
  down = false;
  status = 200;
  /** Sent by the reference relay with 429 and 503. */
  retryAfter: string | null = null;
  /** The registration secret this relay wants for a first push, if any. */
  secret: string | null = null;
  registered = false;
  requests: {
    path: string;
    wait: number;
    auth: string | null;
    registration: string | null;
    at: number;
  }[] = [];
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
      registration: new Headers(init.headers).get("x-gsp-registration"),
      at: Date.now(),
    });
    this.active[kind] += 1;
    this.maxActive[kind] = Math.max(this.maxActive[kind], this.active[kind]);
    try {
      await sleep(1);
      if (init.signal?.aborted) throw new DOMException("aborted", "AbortError");
      if (this.down) throw new TypeError(`fetch failed for ${TOKEN}`);
      if (this.status !== 200)
        return new Response("no", {
          status: this.status,
          headers: this.retryAfter ? { "retry-after": this.retryAfter } : {},
        });
      const body = new Uint8Array(init.body as Uint8Array);
      if (kind === "push") {
        if (!this.registered && this.secret !== null) {
          const sent = new Headers(init.headers).get("x-gsp-registration");
          if (sent !== this.secret) return new Response("no", { status: 403 });
        }
        this.registered = true;
        this.log.push(...body);
        this.release();
        return new Response(new Uint8Array([body.length]));
      }
      const after = body[0]!;
      // Beyond the head: answered at once with the head (here: marker 2).
      if (after > this.log.length) return new Response(new Uint8Array([2]));
      if (wait > 0 && this.log.length <= after)
        await new Promise<void>((resolve, reject) => {
          const aborted = () =>
            reject(new DOMException("aborted", "AbortError"));
          if (init.signal?.aborted) return aborted();
          this.held.push(resolve);
          setTimeout(resolve, wait); // the long poll runs out
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

function setup(options: { online?: boolean; registrationToken?: string } = {}) {
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
    relayUrl: "https://relay.test/base/",
    ...(options.registrationToken === undefined
      ? {}
      : { registrationToken: options.registrationToken }),
    resyncMinMs: 20,
    resyncMaxMs: 80,
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

describe("parseRelayUrl", () => {
  it("accepts https URLs, strips trailing slashes and names the origin", () => {
    expect(parseRelayUrl("https://relay.example.org/")).toEqual({
      url: "https://relay.example.org",
      origin: "https://relay.example.org",
    });
    expect(parseRelayUrl("HTTPS://Relay.Example.org:443/gsp//")).toEqual({
      url: "https://relay.example.org/gsp",
      origin: "https://relay.example.org",
    });
    expect(parseRelayUrl("https://relay.example.org:8443").origin).toBe(
      "https://relay.example.org:8443",
    );
  });
  it("accepts plain http for this machine only", () => {
    for (const ok of [
      "http://localhost:8787",
      "http://127.0.0.1:8787/",
      "http://[::1]:8787",
    ])
      expect(parseRelayUrl(ok).url, ok).toBe(ok.replace(/\/$/, ""));
    for (const bad of [
      "http://relay.example.org",
      "http://192.168.1.10:8787",
      "http://localhost.example.org",
      "http://127.0.0.1.example.org",
    ])
      expect(() => parseRelayUrl(bad), bad).toThrow(
        /must use https:\/\/ \(plain http:\/\/ is accepted for localhost/,
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
      expect(() => parseRelayUrl(bad), bad).toThrow(SiafuError);
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
    // The token asked of the engine is the one for this relay's origin.
    expect(new Set(engine.origins)).toEqual(new Set(["https://relay.test"]));
    expect(relay.requests.every((r) => r.registration === null)).toBe(true);
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
    expect(loop.state().lastError).toMatch(/relay answered 401/);
    await sleep(60);
    // Two loops, at most the first try plus a few retries at the 20-40 ms ceiling.
    expect(relay.requests.length).toBeLessThanOrEqual(8);
    expect(engine.outbox).toEqual([1]);
    await loop.stop();
  });

  it("waits as long as the relay asks with 503 and 429", async () => {
    const { engine, relay, loop } = setup();
    relay.status = 503;
    relay.retryAfter = "0.05"; // seconds
    engine.outbox.push(1);
    loop.start();
    await until(() => loop.state().lastError !== null);
    expect(loop.state().lastError).toBe(
      "the relay is busy (relay answered 503)",
    );
    await sleep(200);
    const refusedRequests = relay.requests.slice();
    relay.status = 200;
    await until(() => engine.outbox.length === 0);
    for (const path of ["/base/v0/push", "/base/v0/pull"]) {
      const refused = refusedRequests
        .filter((r) => r.path === path)
        .map((r) => r.at);
      expect(refused.length).toBeGreaterThanOrEqual(2);
      // Never sooner than asked (50 ms), never much later than 1.5 times that.
      for (let i = 1; i < refused.length; i++) {
        expect(refused[i]! - refused[i - 1]!).toBeGreaterThanOrEqual(49);
        expect(refused[i]! - refused[i - 1]!).toBeLessThan(160);
      }
    }
    await loop.stop();
  });

  it("sends the registration secret until the group is registered, and again after a relay reset", async () => {
    const { engine, relay, loop } = setup({ registrationToken: "s3cret" });
    relay.secret = "s3cret";
    engine.outbox.push(1, 2, 3, 4);
    loop.start();
    await until(() => engine.outbox.length === 0 && engine.cursor === 4);
    const pushes = () => relay.requests.filter((r) => r.path.endsWith("/push"));
    expect(pushes().map((r) => r.registration)).toEqual(["s3cret", null]);
    expect(
      relay.requests
        .filter((r) => r.path.endsWith("/pull"))
        .every((r) => r.registration === null),
    ).toBe(true);

    // The relay loses everything; a new write is waiting.
    relay.log = [];
    relay.registered = false;
    engine.outbox.push(5);
    loop.notifyLocalWrite();
    await until(() => relay.log.includes(5) && engine.outbox.length === 0);
    // Whichever came first, the pull that saw the reset or a push refused
    // with 403: the secret was sent again, and not after that.
    const later = pushes().slice(2);
    expect(later.filter((r) => r.registration === "s3cret")).toHaveLength(1);
    expect(later.at(-1)!.registration).toBe("s3cret");
    expect(JSON.stringify(loop.state())).not.toContain("s3cret");
    await loop.stop();
  });

  it("reports a relay that wants a registration secret it was not given", async () => {
    const { engine, relay, loop } = setup();
    relay.secret = "s3cret";
    engine.outbox.push(1);
    loop.start();
    const failure = await loop.pushNow().then(
      () => null,
      (e: Error & { status?: number }) => e,
    );
    expect(failure?.status).toBe(403);
    expect(failure?.message).toMatch(/requires a registration token/);
    expect(loop.state().lastError).toMatch(/requires a registration token/);
    expect(relay.log).toEqual([]);
    await loop.stop();
  });

  it("resyncs when the engine asks, with growing delays, and stops when it no longer asks", async () => {
    const { engine, relay, loop } = setup();
    relay.log.push(1, 2, 3);
    engine.stuck = true;
    loop.start();
    await until(() => engine.resyncs.length >= 4, 3_000);
    const gaps = engine.resyncs.slice(1).map((t, i) => t - engine.resyncs[i]!);
    // 20 ms, then 40, then 80 (the ceiling): never a spin.
    expect(gaps[0]).toBeGreaterThanOrEqual(35);
    expect(gaps[1]).toBeGreaterThanOrEqual(75);
    expect(gaps[2]).toBeGreaterThanOrEqual(75);
    // Each resync read the log from the start again.
    expect(engine.ingested.length).toBeGreaterThanOrEqual(12);
    expect(relay.requests.length / engine.resyncs.length).toBeLessThan(8);

    // The cause goes away: at most one more replay, then no further resync.
    engine.stuck = false;
    const count = engine.resyncs.length;
    await sleep(300);
    expect(engine.resyncs.length).toBeLessThanOrEqual(count + 1);
    expect(engine.needsResync).toBe(false);
    await until(() => relay.requests.at(-1)!.wait === 5_000);
    await loop.stop();
  });

  it("uploads writes held since opening after the first full read of the relay, or once the relay proves unreachable", async () => {
    // Reachable: the held writes go out only after the pull reached the head.
    const up = setup();
    up.relay.log.push(101, 102, 103);
    up.engine.held.push(1, 2);
    up.loop.start();
    await until(() => up.relay.log.includes(2));
    expect(up.engine.releases).toBe(0);
    const firstPush = up.relay.requests.findIndex((r) =>
      r.path.endsWith("/push"),
    );
    const pullsBefore = up.relay.requests
      .slice(0, firstPush)
      .filter((r) => r.path.endsWith("/pull"));
    // 3 items, two per frame: both frames were read first (the idle long
    // poll that follows may already be open as well).
    expect(pullsBefore.length).toBeGreaterThanOrEqual(2);
    expect(up.seen.persisted).toBeGreaterThan(0);
    await up.loop.stop();

    // Unreachable: they are not held for as long as that lasts.
    const down = setup();
    down.relay.down = true;
    down.engine.held.push(1, 2);
    down.loop.start();
    await until(() => down.engine.outbox.length === 2);
    expect(down.engine.releases).toBeGreaterThan(0);
    expect(down.relay.log).toEqual([]);
    down.relay.down = false;
    await until(() => down.relay.log.length === 2);
    await down.loop.stop();
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
