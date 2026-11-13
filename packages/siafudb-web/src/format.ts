/**
 * Encrypted snapshot container (format version 1).
 *
 * ```
 * offset size  field
 * 0      4     magic "SFDB"
 * 4      1     format version (1)
 * 5      1     cipher-suite id (see {@link CipherSuite})
 * 6      1     nonce length in bytes (must match the suite)
 * 7      1     flags (must be 0)
 * 8      8     write counter, u64 big-endian, strictly increasing per database
 * 16     N     nonce (random; 12 bytes for AES-256-GCM)
 * 16+N   ...   ciphertext || 16-byte authentication tag
 * ```
 *
 * AAD (additional authenticated data) =
 * `u16be(len(name)) || utf8(name) || version || suite || u64be(counter)`.
 * Binding the database name stops a blob being moved to another database;
 * binding the counter and suite stops header edits (rollback labels, suite
 * downgrade) from going unnoticed. The counter is *also* checked against a
 * high-water mark kept outside the snapshot store (see `store.ts`) so that
 * replaying an older, perfectly valid blob is rejected.
 *
 * Nonces are random 96-bit values; each database has its own data key and the
 * number of writes per key stays far below the 2^32 bound NIST SP 800-38D
 * allows for random-nonce AES-GCM.
 */
import { FormatError, TamperError, UnsupportedCipherError } from "./errors.js";

/**
 * Numeric cipher-suite ids stored in the header. Ids are append-only; never
 * reuse one.
 *
 * - `0`: reserved (never valid; guards against zeroed headers).
 * - `1`: XChaCha20-Poly1305, 24-byte nonce. This is the planned default of
 *   the Rust-side page store (`siafudb-store`). WebCrypto has no ChaCha20, so
 *   this package recognises the id and refuses it with
 *   {@link UnsupportedCipherError} instead of mis-parsing the blob.
 * - `2`: AES-256-GCM, 12-byte nonce, 128-bit tag (implemented here).
 * - `3..255`: reserved.
 */
export const CipherSuite = {
  XCHACHA20_POLY1305: 1,
  AES_256_GCM: 2,
} as const;
export type CipherSuiteId = (typeof CipherSuite)[keyof typeof CipherSuite];

export const FORMAT_VERSION = 1;
export const MAGIC = new Uint8Array([0x53, 0x46, 0x44, 0x42]); // "SFDB"
const FIXED_HEADER = 16;
const AES_GCM_NONCE = 12;
const TAG_BITS = 128;
const TAG_BYTES = TAG_BITS / 8;
const te = new TextEncoder();

/** Parsed (unauthenticated) header. */
export interface SnapshotHeader {
  version: number;
  suite: number;
  counter: number;
  nonce: Uint8Array;
  /** Byte offset where ciphertext begins. */
  bodyOffset: number;
}

function nonceLengthFor(suite: number): number {
  if (suite === CipherSuite.AES_256_GCM) return AES_GCM_NONCE;
  if (suite === CipherSuite.XCHACHA20_POLY1305) return 24;
  throw new FormatError(`unknown cipher suite id ${suite}`);
}

function u64be(n: number): Uint8Array {
  if (!Number.isSafeInteger(n) || n < 0)
    throw new RangeError("counter must be a non-negative safe integer");
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, BigInt(n));
  return b;
}

/** AAD binding database name, format version, suite and write counter. */
export function buildAad(
  name: string,
  version: number,
  suite: number,
  counter: number,
): Uint8Array {
  const nameBytes = te.encode(name);
  if (nameBytes.length > 0xffff) throw new RangeError("database name too long");
  const aad = new Uint8Array(2 + nameBytes.length + 2 + 8);
  new DataView(aad.buffer).setUint16(0, nameBytes.length);
  aad.set(nameBytes, 2);
  aad[2 + nameBytes.length] = version;
  aad[3 + nameBytes.length] = suite;
  aad.set(u64be(counter), 4 + nameBytes.length);
  return aad;
}

/** Parse the header without authenticating it. Throws {@link FormatError} if malformed. */
export function parseHeader(blob: Uint8Array): SnapshotHeader {
  if (blob.length < FIXED_HEADER)
    throw new FormatError("blob too short to be a SiafuDB snapshot");
  for (let i = 0; i < 4; i++)
    if (blob[i] !== MAGIC[i])
      throw new FormatError("bad magic: not a SiafuDB snapshot");
  const version = blob[4]!;
  if (version !== FORMAT_VERSION)
    throw new FormatError(`unsupported format version ${version}`);
  const suite = blob[5]!;
  const nonceLen = blob[6]!;
  if (blob[7] !== 0) throw new FormatError("unknown header flags");
  if (suite === 0) throw new FormatError("cipher suite 0 is reserved");
  if (nonceLen !== nonceLengthFor(suite))
    throw new FormatError("nonce length does not match cipher suite");
  const big = new DataView(
    blob.buffer,
    blob.byteOffset,
    blob.byteLength,
  ).getBigUint64(8);
  if (big > BigInt(Number.MAX_SAFE_INTEGER))
    throw new FormatError("write counter out of range");
  const bodyOffset = FIXED_HEADER + nonceLen;
  if (blob.length < bodyOffset + TAG_BYTES)
    throw new FormatError("blob truncated");
  return {
    version,
    suite,
    counter: Number(big),
    nonce: blob.slice(FIXED_HEADER, bodyOffset),
    bodyOffset,
  };
}

/** Best-effort counter peek used only to order candidate blobs; never trust it for security. */
export function peekCounter(blob: Uint8Array): number {
  try {
    return parseHeader(blob).counter;
  } catch {
    return -1;
  }
}

export interface EncodeOptions {
  name: string;
  key: CryptoKey;
  counter: number;
  plaintext: Uint8Array;
  /** Defaults to AES-256-GCM, the only suite implemented here. */
  suite?: CipherSuiteId;
}

/** Encrypt `plaintext` into a self-describing container. */
export async function encodeSnapshot(o: EncodeOptions): Promise<Uint8Array> {
  const suite = o.suite ?? CipherSuite.AES_256_GCM;
  if (suite !== CipherSuite.AES_256_GCM)
    throw new UnsupportedCipherError(suite);
  const nonce = crypto.getRandomValues(new Uint8Array(AES_GCM_NONCE));
  const aad = buildAad(o.name, FORMAT_VERSION, suite, o.counter);
  const ct = new Uint8Array(
    await crypto.subtle.encrypt(
      {
        name: "AES-GCM",
        iv: nonce as BufferSource,
        additionalData: aad as BufferSource,
        tagLength: TAG_BITS,
      },
      o.key,
      o.plaintext as BufferSource,
    ),
  );
  const out = new Uint8Array(FIXED_HEADER + nonce.length + ct.length);
  out.set(MAGIC, 0);
  out[4] = FORMAT_VERSION;
  out[5] = suite;
  out[6] = nonce.length;
  out[7] = 0;
  out.set(u64be(o.counter), 8);
  out.set(nonce, FIXED_HEADER);
  out.set(ct, FIXED_HEADER + nonce.length);
  return out;
}

export interface DecodeOptions {
  name: string;
  key: CryptoKey;
  blob: Uint8Array;
}

/**
 * Authenticate and decrypt a container.
 * @throws {FormatError} malformed header
 * @throws {UnsupportedCipherError} header names a suite this build cannot run
 * @throws {TamperError} authentication failed
 */
export async function decodeSnapshot(
  o: DecodeOptions,
): Promise<{ plaintext: Uint8Array; counter: number; suite: number }> {
  const h = parseHeader(o.blob);
  if (h.suite !== CipherSuite.AES_256_GCM)
    throw new UnsupportedCipherError(h.suite);
  const aad = buildAad(o.name, h.version, h.suite, h.counter);
  try {
    const pt = await crypto.subtle.decrypt(
      {
        name: "AES-GCM",
        iv: h.nonce as BufferSource,
        additionalData: aad as BufferSource,
        tagLength: TAG_BITS,
      },
      o.key,
      o.blob.subarray(h.bodyOffset) as BufferSource,
    );
    return {
      plaintext: new Uint8Array(pt),
      counter: h.counter,
      suite: h.suite,
    };
  } catch (cause) {
    throw Object.assign(new TamperError(), { cause });
  }
}
