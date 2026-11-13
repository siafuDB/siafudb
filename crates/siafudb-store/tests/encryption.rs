//! EncryptedPageStore: tamper, AAD binding, wrong key, KEK providers.

mod common;

use common::*;
use siafudb_store::{
    AeadSuite, Argon2Params, CallbackKek, EncryptOptions, EncryptedPageStore, KekProvider,
    KeyHeader, MemPageStore, PageStore, PassphraseKek, RawKek, StoreError, UnwrappedKek,
    generate_dek,
};
use zeroize::Zeroizing;

fn one_page_store() -> (MemPageStore, Vec<u8>) {
    let inner = MemPageStore::new(PPS);
    let e = encrypt(inner.clone());
    let plain = bytes(42, LPS);
    e.write_page(0, &plain).unwrap();
    (inner, plain)
}

/// Flip every bit of a ciphertext page, one at a time: each flip must be an
/// error, never plaintext (correct or otherwise).
#[test]
fn every_bit_flip_in_a_page_is_rejected() {
    let (inner, plain) = one_page_store();
    let e = reopen_encrypted(inner.clone()).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, plain);
    let base = 2 * PPS; // logical page 0 = physical page 2
    let mut flips = 0;
    for byte in 0..PPS {
        for bit in 0..8 {
            inner.with_bytes_mut(|b| b[base + byte] ^= 1 << bit);
            let r = e.read_page(0, &mut buf);
            assert!(
                matches!(r, Err(StoreError::Integrity(0))),
                "byte {byte} bit {bit}: {r:?}"
            );
            assert!(buf.iter().all(|&x| x == 0), "no plaintext on failure");
            inner.with_bytes_mut(|b| b[base + byte] ^= 1 << bit);
            flips += 1;
        }
    }
    assert_eq!(flips, PPS * 8);
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, plain);
}

#[test]
fn pages_are_bound_to_slot_and_database() {
    let inner = MemPageStore::new(PPS);
    let e = encrypt(inner.clone());
    e.write_page(0, &bytes(1, LPS)).unwrap();
    e.write_page(1, &bytes(2, LPS)).unwrap();
    // Copy physical page of logical 0 over logical 1: AAD has page_no.
    inner.with_bytes_mut(|b| {
        let p0 = b[2 * PPS..3 * PPS].to_vec();
        b[3 * PPS..4 * PPS].copy_from_slice(&p0);
    });
    let mut buf = vec![0u8; LPS];
    assert!(matches!(
        e.read_page(1, &mut buf),
        Err(StoreError::Integrity(1))
    ));

    // A page from another database with the same key and slot is rejected
    // (db_id in AAD; here the DEKs also differ).
    let other = MemPageStore::new(PPS);
    let mut o = opts();
    o.db_id = Some([1u8; 16]);
    let e2 = EncryptedPageStore::create(other.clone(), &kek(), o).unwrap();
    e2.write_page(0, &bytes(3, LPS)).unwrap();
    let foreign = other.to_bytes()[2 * PPS..3 * PPS].to_vec();
    inner.with_bytes_mut(|b| b[2 * PPS..3 * PPS].copy_from_slice(&foreign));
    assert!(matches!(
        e.read_page(0, &mut buf),
        Err(StoreError::Integrity(0))
    ));
}

#[test]
fn nonces_are_random_and_versions_increase() {
    let inner = MemPageStore::new(PPS);
    let e = encrypt(inner.clone());
    let page = bytes(5, LPS);
    e.write_page(0, &page).unwrap();
    let a = inner.to_bytes()[2 * PPS..3 * PPS].to_vec();
    e.write_page(0, &page).unwrap();
    let b = inner.to_bytes()[2 * PPS..3 * PPS].to_vec();
    assert_ne!(a[14..38], b[14..38], "same plaintext, fresh nonce");
    assert_ne!(a[38..], b[38..], "ciphertext differs");
    let ver = |p: &[u8]| u64::from_le_bytes(p[6..14].try_into().unwrap());
    assert!(ver(&b) > ver(&a));
    assert_eq!(
        u16::from_le_bytes([a[0], a[1]]),
        AeadSuite::XChaCha20Poly1305.id()
    );
    // Replaying the older authentic copy decrypts at this layer (only the
    // container's root can tell it is stale; see container tests).
    inner.with_bytes_mut(|x| x[2 * PPS..3 * PPS].copy_from_slice(&a));
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
}

#[test]
fn wrong_raw_key_fails_before_decrypting() {
    let (inner, _) = one_page_store();
    let r = EncryptedPageStore::open(inner, &RawKek::new([8u8; 32]));
    assert!(matches!(r, Err(StoreError::WrongKey)), "{r:?}");
}

#[test]
fn passphrase_kek_round_trip_and_wrong_passphrase() {
    let inner = MemPageStore::new(PPS);
    let k = PassphraseKek::with_params(b"correct horse", Argon2Params::INSECURE_TEST);
    let e = EncryptedPageStore::create(inner.clone(), &k, opts()).unwrap();
    e.write_page(0, &bytes(1, LPS)).unwrap();
    drop(e);
    let bad = PassphraseKek::with_params(b"battery staple", Argon2Params::INSECURE_TEST);
    assert!(matches!(
        EncryptedPageStore::open(inner.clone(), &bad),
        Err(StoreError::WrongKey)
    ));
    // Raw KEK on a passphrase store: wrong provider -> WrongKey.
    assert!(matches!(
        EncryptedPageStore::open(inner.clone(), &kek()),
        Err(StoreError::WrongKey)
    ));
    let e = EncryptedPageStore::open(inner, &k).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, bytes(1, LPS));
}

#[test]
fn host_kek_returning_garbage_is_caught_by_key_check() {
    let (inner, plain) = one_page_store();
    // A host "unwrap" that returns 32 bytes that are not the DEK.
    let liar = CallbackKek::new(
        "secure-enclave/v1",
        |_, _| Err(StoreError::Io("unused".into())),
        |_, _| Ok(Zeroizing::new(vec![0xAA; 32])),
    );
    assert!(matches!(
        EncryptedPageStore::open(inner.clone(), &liar),
        Err(StoreError::WrongKey)
    ));
    assert!(matches!(
        EncryptedPageStore::open(inner.clone(), &UnwrappedKek::new(Zeroizing::new([1u8; 32]))),
        Err(StoreError::WrongKey)
    ));
    let e = reopen_encrypted(inner).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, plain);
}

/// The async-host (WebCrypto) flow: host generates/wraps the DEK itself,
/// later reads the header, unwraps itself, and opens with the raw DEK.
#[test]
fn async_host_flow() {
    let host_kek = RawKek::new([3u8; 32]); // stands in for crypto.subtle
    let o = EncryptOptions {
        db_id: Some([4u8; 16]),
        ..opts()
    };
    let dek = generate_dek().unwrap();
    let ctx = EncryptedPageStore::<MemPageStore>::wrap_context_for(&o).unwrap();
    let wrapped = host_kek.wrap(&dek, &ctx).unwrap();
    let inner = MemPageStore::new(PPS);
    let e = EncryptedPageStore::create_with_wrapped(inner.clone(), dek, wrapped, o).unwrap();
    e.write_page(0, &bytes(9, LPS)).unwrap();
    drop(e);

    let h = KeyHeader::read(&inner).unwrap();
    let dek = host_kek.unwrap(&h.wrapped, &h.wrap_context()).unwrap();
    let e = EncryptedPageStore::open(inner, &UnwrappedKek::new(dek)).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, bytes(9, LPS));
}

#[test]
fn header_copy_damage_and_tamper() {
    let (inner, plain) = one_page_store();
    // Damage copy 0: copy 1 still opens.
    inner.with_bytes_mut(|b| b[10] ^= 0xFF);
    let e = reopen_encrypted(inner.clone()).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, plain);
    drop(e);
    // Damage both: clean error, no decrypt attempted.
    inner.with_bytes_mut(|b| b[PPS + 10] ^= 0xFF);
    assert!(matches!(
        reopen_encrypted(inner.clone()),
        Err(StoreError::Corrupt(_))
    ));

    // A forged header (db_id changed, checksum recomputed) fails the key
    // check: the KCV binds the header fields to the DEK.
    let (inner, _) = one_page_store();
    for slot in 0..2 {
        inner.with_bytes_mut(|b| {
            let p = &mut b[slot * PPS..(slot + 1) * PPS];
            p[20] ^= 1; // inside db_id
            let sum = blake3_hash(&p[..PPS - 32]);
            p[PPS - 32..].copy_from_slice(&sum);
        });
    }
    assert!(matches!(reopen_encrypted(inner), Err(StoreError::WrongKey)));
}

/// The header checksum is unkeyed BLAKE3 (it detects torn writes, not
/// forgery); recomputed here to play the attacker.
fn blake3_hash(b: &[u8]) -> [u8; 32] {
    *blake3::hash(b).as_bytes()
}

#[test]
fn rewrap_changes_kek() {
    let (inner, plain) = one_page_store();
    let e = reopen_encrypted(inner.clone()).unwrap();
    let new = PassphraseKek::with_params(b"new pass", Argon2Params::INSECURE_TEST);
    e.rewrap(&new).unwrap();
    drop(e);
    assert!(matches!(
        reopen_encrypted(inner.clone()),
        Err(StoreError::WrongKey)
    ));
    let e = EncryptedPageStore::open(inner, &new).unwrap();
    let mut buf = vec![0u8; LPS];
    e.read_page(0, &mut buf).unwrap();
    assert_eq!(buf, plain);
}

#[test]
fn unknown_suite_id_is_reported() {
    assert!(matches!(
        AeadSuite::from_id(2),
        Err(StoreError::UnsupportedSuite(2))
    ));
    assert!(matches!(
        AeadSuite::from_id(0),
        Err(StoreError::UnsupportedSuite(0))
    ));
}

#[test]
fn plaintext_does_not_appear_in_physical_pages() {
    let inner = MemPageStore::new(PPS);
    let e = encrypt(inner.clone());
    let marker = b"SECRET-MARKER-123";
    let mut page = vec![0u8; LPS];
    page[..marker.len()].copy_from_slice(marker);
    e.write_page(0, &page).unwrap();
    assert!(!inner.to_bytes().windows(marker.len()).any(|w| w == marker));
}
