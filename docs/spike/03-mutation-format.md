# 3. Do the mutation and change-log formats carry algorithm version tags?

**No. In fact nothing is signed or verified anywhere in the repo yet.** That is
the good news for crypto-agility: there is no signed data to migrate, so the
format can be fixed now at zero migration cost. Not a single mutation exists in
the wild.

## Evidence

- `ed25519-dalek` is a declared dependency of `siafudb-core` and is **never
  imported**. No `sign`/`verify` call exists.
- `Mutation.signature` is `Option<Vec<u8>>`: no algorithm, no key id, no format
  version (`siafudb-sync/src/protocol/mutation.rs`).
- `NodeIdentity` is a placeholder holding only a `fragment_id`
  (`siafudb-core/src/identity/mod.rs`; the file says so).
- Hashing: SHA-256 is hard-coded in the PII-strip path (`transform.rs:323`),
  truncated to 8 bytes, output prefixed `anon_`. Untagged.
- Discovery announcement has `protocol_version: String` but no suite offer.
- `SyncCursor` and vector-clock logic exist (not tested in this spike), but
  `protocol/changelog.rs` is `#![allow(dead_code)]` and in-memory.
- **What actually replicates is raw query text.** `SiafuDB::execute` appends
  `ChangeLogEntry { query: String, rows_affected }`, and `InProcessReplicator`
  re-runs `entry.query` on the destination. A signature over a query string
  cannot be content-addressed or trusted: replaying `CREATE` twice makes two
  nodes, the same text can mean different things under different engine
  versions (0.6 changed query semantics; the engine is now our frozen fork, so this risk shrinks but a signed query string is still not content-addressable), and `rows_affected` is
  `result.rows().len()`, which is not an affected count.
- Both change logs are in-memory: restarting loses them.

## The problem to solve first: what is a mutation?

Algorithm tags are cheap. The real design decision is **where structured
mutations come from**. Options:

| Source                                                                                                              | Native/mobile            | Browser (WASM)                                                                                                                                                                                                                                 | Notes                                                                                                                                |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| Engine CDC (`cdc` feature, `changes_between`, `history_since`)                                                      | Yes (in the engine fork) | **Yes, measured**: upstream's README says the WASM engine has no change tracking, but the `cdc` feature compiles for wasm and captured 3 `Create` events with before/after images in headless Chromium ([01](01-wasm.md#browser-measurements)) | Least code. Behaviour is ours to maintain (the engine is a frozen fork, ADR-0001), so capture can be made identical on every target. |
| SiafuDB-level structured write API (typed `create_vertex`, `set_property`, `create_edge`) and parameterised queries | Yes                      | Yes                                                                                                                                                                                                                                            | Most control, identical on every target. You lose "arbitrary Cypher writes are synced" unless writes are routed through it.          |
| Diff before/after a transaction                                                                                     | Yes                      | Yes                                                                                                                                                                                                                                            | Expensive. Not recommended.                                                                                                          |

**Recommendation:** structured write API as the supported, signed path (works
everywhere, deterministic, signable); CDC as an additional capture for arbitrary
Cypher on native/mobile if wanted. **This is your decision**: it determines
whether "write Cypher, it syncs" is promised in the browser.

## Proposed format (for sign-off; not implemented)

Principles: numeric registry ids (never free strings); every signature, key and
hash self-describing; multiple signatures allowed (hybrid = two entries, both
must verify); deterministic encoding; content-addressed id; author is a DID.

```rust
/// Registry ids. Append-only. 0 is reserved/invalid. Unknown id => the
/// mutation is retained and marked Unverifiable, never silently dropped.
#[repr(u16)] pub enum SigAlg  { Ed25519 = 1, MlDsa65 = 2 }          // hybrid = two Signature entries
#[repr(u16)] pub enum HashAlg { Blake3_256 = 1, Sha3_256 = 2, Sha256 = 3 }
#[repr(u16)] pub enum KemAlg  { X25519 = 1, MlKem768 = 2 }          // sync key exchange: both, combined
#[repr(u16)] pub enum AeadAlg { XChaCha20Poly1305 = 1, Aes256Gcm = 2 }

pub struct Multihash { pub alg: HashAlg, pub digest: Vec<u8> }

pub struct Signature {
    pub alg: SigAlg,
    pub key_id: Multihash,        // fingerprint of the public key
    pub bytes: Vec<u8>,
}

pub struct MutationEnvelope {
    /// Envelope version. Bumped only for structural changes, never for algs.
    pub format: u16,                          // = 1 (this proposal)
    /// Content address: hash of `canonical(body)`. Algorithm is in the id,
    /// so ids minted under an old hash stay valid forever.
    pub id: Multihash,
    pub body: MutationBody,                   // today's Mutation fields minus signature
    pub author: String,                       // DID, e.g. did:key:z6Mk... / did:web:...
    pub hash_alg: HashAlg,                    // alg used for `id`
    pub sigs: Vec<Signature>,                 // empty only if fragment policy allows
}
```

Rules:

1. **Canonical bytes** = deterministic CBOR (RFC 8949 section 4.2) of `body`.
   JSON is not canonical (`serde_json::Map` ordering, float formatting), so the
   current `serde_json` bodies cannot be signed safely.
2. **Signed message** is the concatenation of a domain label
   (`"siafudb/mutation/v1"`), `format`, `hash_alg`, `id` and `author`.
   Domain separation stops cross-protocol signature reuse.
3. **Downgrade resistance.** The author's key set (DID document) declares which
   suites are acceptable and from when. A verifier requires _every signature the
   author's policy demands_, so an attacker cannot strip the ML-DSA signature and
   present Ed25519 alone. Initial policy: hybrid Ed25519 + ML-DSA-65 required for
   Network fragments; Ed25519 alone allowed for Personal/local-only.
4. **Migrating algorithms.** Never rewrite history. Add a new signature entry
   (re-attestation envelope referencing the old `id`) under the new algorithm.
5. **Sync handshake.** Replace `protocol_version: String` with a list of
   offered suites and negotiate. Session keys: X25519 and ML-KEM-768
   secrets combined through a KDF (hybrid, so harvest-now-decrypt-later is
   covered while either is unbroken).
6. **At rest.** XChaCha20-Poly1305 default (AES-256-GCM allowed), id in the page
   header (see 02). BLAKE3-256 default hash, SHA3-256 alternate; SHA-256 kept for
   interop.
7. **DIDs.** Replace `NodeIdentity { fragment_id }` with `{ fragment_id, did }`.
   Pods are sync peers addressed by DID. Chains are optional anchors: publish the
   Merkle root of a checkpoint, not data. Out of scope until asked.

**The smallest change that locks the door shut** (do this even if nothing else
is approved): replace `signature: Option<Vec<u8>>` with
`sigs: Vec<Signature>` where `Signature { alg: u16, key_id, bytes }`, and add
`format: u16` to `Mutation`. Additive in practice, since nothing writes either
field yet. Everything else can follow.

## Crate availability (compiles on wasm32, iOS and Android; correctness untested)

`ml-kem` 0.3.4, `ml-dsa` 0.1.2, `blake3` 1.8.7, `sha3` 0.12, `chacha20poly1305`
0.11, `x25519-dalek` 3, `ed25519-dalek` 3. Two non-obvious requirements,
discovered by failing builds:

- **`blake3` must use `features = ["pure"]`**: its default build script compiles
  C/asm SIMD and fails for iOS/Android in this environment. With `pure` the
  tree stays Rust-only, matching the README's "no C" claim.
- **`getrandom` needs `wasm_js`** on `wasm32-unknown-unknown` (the PQ crates and
  Grafeo's `embedded` profile both pull `getrandom 0.4`).

`ml-dsa` is a 0.1 release. I did not check audit status, constant-time
properties, or performance on a phone. _Unverified; check before shipping it as
the sole signature._ The hybrid-with-Ed25519 rule above is the mitigation.

## Effort

- Envelope types, registry, canonical CBOR, sign/verify with Ed25519 + ML-DSA,
  tests: about 1 week.
- Replace query-text capture with structured mutations and make replication
  idempotent (apply by `id`, skip seen): 1-2 weeks, depending on the capture
  decision above.
- Persist the log through `PageStore` (02): folded into Phase 1.
- Hybrid KEM handshake: separate, after GSPA exists. Out of scope for the spike.

Reproduce crate builds: [`probes/pq`](probes/pq/).
