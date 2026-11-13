import { describe, expect, it } from "vitest";
import {
  FormatError,
  KeyMissingError,
  RollbackError,
  TamperError,
  UnsupportedCipherError,
} from "../../src/errors.js";
import {
  CipherSuite,
  buildAad,
  decodeSnapshot,
  encodeSnapshot,
  parseHeader,
} from "../../src/format.js";
import { EncryptedSnapshotStore, MemoryBackend } from "../../src/store.js";
import { MemoryKeyVault } from "../../src/vault.js";

const te = new TextEncoder();
const td = new TextDecoder();
const aesKey = () =>
  crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, [
    "encrypt",
    "decrypt",
  ]);
const MARKER = "SECRET-PLAINTEXT-ZEBRA-7731";

function contains(hay: Uint8Array, needle: string): boolean {
  return Buffer.from(hay).includes(Buffer.from(needle));
}

describe("container format", () => {
  it("round-trips and writes a well-formed header", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "db",
      key,
      counter: 7,
      plaintext: te.encode(MARKER),
    });
    expect(contains(blob, MARKER)).toBe(false);
    expect(td.decode(blob.subarray(0, 4))).toBe("SFDB");
    const h = parseHeader(blob);
    expect(h).toMatchObject({
      version: 1,
      suite: CipherSuite.AES_256_GCM,
      counter: 7,
    });
    expect(h.nonce.length).toBe(12);
    const r = await decodeSnapshot({ name: "db", key, blob });
    expect(td.decode(r.plaintext)).toBe(MARKER);
    expect(r.counter).toBe(7);
  });

  it("uses a fresh random nonce for every write", async () => {
    const key = await aesKey();
    const seen = new Set<string>();
    for (let i = 0; i < 200; i++) {
      const b = await encodeSnapshot({
        name: "db",
        key,
        counter: i + 1,
        plaintext: new Uint8Array(4),
      });
      seen.add(Buffer.from(parseHeader(b).nonce).toString("hex"));
    }
    expect(seen.size).toBe(200);
  });

  it("AAD is unambiguous across name/version/suite/counter", () => {
    expect(buildAad("ab", 1, 2, 1)).not.toEqual(buildAad("a", 1, 2, 1));
    expect(buildAad("a", 1, 2, 1)).not.toEqual(buildAad("a", 1, 2, 2));
    expect(buildAad("a", 1, 2, 1)).not.toEqual(buildAad("a", 1, 1, 1));
  });

  it("rejects every single-byte flip anywhere in the blob", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "db",
      key,
      counter: 3,
      plaintext: te.encode("hello world"),
    });
    let rejected = 0;
    for (let i = 0; i < blob.length; i++) {
      const t = blob.slice();
      t[i]! ^= 0x01;
      await expect(
        decodeSnapshot({ name: "db", key, blob: t }),
      ).rejects.toBeInstanceOf(Error);
      rejected++;
    }
    expect(rejected).toBe(blob.length);
  });

  it("flipping the counter in the header is a tamper error, not a silent relabel", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "db",
      key,
      counter: 3,
      plaintext: te.encode("x"),
    });
    blob[15]! = 9; // counter low byte 3 -> 9
    await expect(
      decodeSnapshot({ name: "db", key, blob }),
    ).rejects.toBeInstanceOf(TamperError);
  });

  it("rejects a blob presented under a different database name or key", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "a",
      key,
      counter: 1,
      plaintext: te.encode("x"),
    });
    await expect(
      decodeSnapshot({ name: "b", key, blob }),
    ).rejects.toBeInstanceOf(TamperError);
    await expect(
      decodeSnapshot({ name: "a", key: await aesKey(), blob }),
    ).rejects.toBeInstanceOf(TamperError);
  });

  it("rejects truncated, bad-magic, bad-version and reserved-suite headers as FormatError", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "db",
      key,
      counter: 1,
      plaintext: te.encode("x"),
    });
    await expect(
      decodeSnapshot({ name: "db", key, blob: blob.slice(0, 20) }),
    ).rejects.toBeInstanceOf(FormatError);
    const badMagic = blob.slice();
    badMagic[0] = 0;
    await expect(
      decodeSnapshot({ name: "db", key, blob: badMagic }),
    ).rejects.toBeInstanceOf(FormatError);
    const badVer = blob.slice();
    badVer[4] = 9;
    await expect(
      decodeSnapshot({ name: "db", key, blob: badVer }),
    ).rejects.toBeInstanceOf(FormatError);
    const suite0 = blob.slice();
    suite0[5] = 0;
    await expect(
      decodeSnapshot({ name: "db", key, blob: suite0 }),
    ).rejects.toBeInstanceOf(FormatError);
  });

  it("recognises XChaCha20-Poly1305 (id 1) and refuses it explicitly", async () => {
    const key = await aesKey();
    const fake = new Uint8Array(16 + 24 + 16);
    fake.set(te.encode("SFDB"));
    fake[4] = 1;
    fake[5] = CipherSuite.XCHACHA20_POLY1305;
    fake[6] = 24;
    await expect(
      decodeSnapshot({ name: "db", key, blob: fake }),
    ).rejects.toBeInstanceOf(UnsupportedCipherError);
    await expect(
      encodeSnapshot({
        name: "db",
        key,
        counter: 1,
        plaintext: new Uint8Array(1),
        suite: CipherSuite.XCHACHA20_POLY1305,
      }),
    ).rejects.toBeInstanceOf(UnsupportedCipherError);
  });

  it("rejects a downgraded suite label", async () => {
    const key = await aesKey();
    const blob = await encodeSnapshot({
      name: "db",
      key,
      counter: 1,
      plaintext: te.encode("x"),
    });
    // Relabel as suite 1 with a (wrong) 12-byte nonce length: format error, never a successful decrypt.
    blob[5] = 1;
    await expect(
      decodeSnapshot({ name: "db", key, blob }),
    ).rejects.toBeInstanceOf(FormatError);
  });
});

describe("encrypted snapshot store", () => {
  const setup = () => {
    const backend = new MemoryBackend();
    const vault = new MemoryKeyVault();
    return {
      backend,
      vault,
      store: () => new EncryptedSnapshotStore("db", backend, vault),
    };
  };

  it("new database loads as null; saves round-trip with strictly increasing counters", async () => {
    const { backend, store } = setup();
    const s = store();
    expect(await s.load()).toBeNull();
    expect(await s.save(te.encode("one"))).toBe(1);
    expect(await s.save(te.encode("two"))).toBe(2);
    expect(await s.save(te.encode("three"))).toBe(3);
    expect(backend.slots.some((b) => contains(b, "three"))).toBe(false);
    const r = store();
    expect(td.decode((await r.load())!)).toBe("three");
    expect(r.writeCounter).toBe(3);
    expect(await r.save(te.encode("four"))).toBe(4);
  });

  it("stores no plaintext in the backend", async () => {
    const { backend, store } = setup();
    const s = store();
    await s.save(te.encode(MARKER));
    await s.save(te.encode(MARKER + "2"));
    for (const b of backend.slots) expect(contains(b, MARKER)).toBe(false);
  });

  it("detects rollback: replaying an older valid snapshot is rejected", async () => {
    const { backend, store } = setup();
    const s = store();
    await s.save(te.encode("v1")); // slot 1
    const old = backend.slots[1]!.slice();
    await s.save(te.encode("v2")); // slot 0
    await s.save(te.encode("v3")); // slot 1
    // attacker restores the v1 file over the newest slot and removes the other
    backend.slots = [new Uint8Array(0), old].filter((b) => b.length > 0);
    const err = await store()
      .load()
      .catch((e: unknown) => e);
    expect(err).toBeInstanceOf(RollbackError);
    expect((err as RollbackError).found).toBe(1);
    expect((err as RollbackError).expectedAtLeast).toBe(3);
  });

  it("detects rollback when the snapshot is deleted after data was committed", async () => {
    const { backend, store } = setup();
    await store().save(te.encode("v1"));
    backend.slots = [];
    await expect(store().load()).rejects.toBeInstanceOf(RollbackError);
  });

  it("tampered newest snapshot is rejected even when an older valid slot remains", async () => {
    const { backend, store } = setup();
    const s = store();
    await s.save(te.encode("v1"));
    await s.save(te.encode("v2"));
    const newest = backend.slots[0]!; // counter 2 -> slot 0
    newest[newest.length >> 1]! ^= 1;
    await expect(store().load()).rejects.toBeInstanceOf(TamperError);
  });

  it("survives a torn write: a half-written newest slot falls back to the previous snapshot", async () => {
    const { backend, vault, store } = setup();
    const s = store();
    await s.save(te.encode("v1"));
    await s.save(te.encode("v2"));
    // Simulate: v3 blob partly written to slot 1 but crashed before the counter commit.
    backend.slots[1] = backend.slots[1]!.slice(0, 30);
    expect(await vault.getCounter("db")).toBe(2);
    expect(td.decode((await store().load())!)).toBe("v2");
  });

  it("recovers when the crash happened between blob write and counter commit", async () => {
    const { backend, vault, store } = setup();
    const s = store();
    await s.save(te.encode("v1"));
    // v2 fully written, vault counter still 1:
    const key = await vault.getDataKey("db", { create: false });
    backend.slots[0] = await encodeSnapshot({
      name: "db",
      key,
      counter: 2,
      plaintext: te.encode("v2"),
    });
    const r = store();
    expect(td.decode((await r.load())!)).toBe("v2");
    expect(await vault.getCounter("db")).toBe(2);
  });

  it("missing data key for an existing snapshot is KeyMissingError, not a silent new key", async () => {
    const { backend, store } = setup();
    await store().save(te.encode("v1"));
    await expect(
      new EncryptedSnapshotStore("db", backend, new MemoryKeyVault()).load(),
    ).rejects.toBeInstanceOf(KeyMissingError);
  });

  it("a snapshot copied to another database name does not load", async () => {
    const { backend, vault, store } = setup();
    await store().save(te.encode("v1"));
    const other = new EncryptedSnapshotStore("other", backend, vault);
    await expect(other.load()).rejects.toBeInstanceOf(KeyMissingError);
  });

  it("destroy wipes blobs, key and counter", async () => {
    const { backend, vault, store } = setup();
    const s = store();
    await s.save(te.encode("v1"));
    await s.destroy();
    expect(backend.slots).toEqual([]);
    expect(await vault.getCounter("db")).toBe(0);
    expect(await store().load()).toBeNull();
  });
});
