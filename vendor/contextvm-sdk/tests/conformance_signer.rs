//! Conformance tests for signer behavior (hex `from_sk`, `generate`, NIP-44, signing).
//!
//! Same layout as `conformance_wire_format.rs`; scenarios follow the TS SDK
//! `private-key-signer.test.ts` alongside `src/signer/mod.rs` / `src/encryption/mod.rs`.

use contextvm_sdk::encryption::{decrypt_nip44, encrypt_nip44};
use contextvm_sdk::signer::{self, Keys};
use nostr_sdk::prelude::*;

/// Secret `1`, x-only pubkey of secp256k1 `G`.
const FIXTURE_SK_HEX: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const FIXTURE_PK_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

fn fixture_keys() -> Keys {
    signer::from_sk(FIXTURE_SK_HEX).expect("fixture SK hex parses")
}

// ── Key derivation ───────────────────────────────────────────────────────────

#[test]
fn signer_generates_keypair_from_secret_key() {
    let keys = fixture_keys();
    assert_eq!(keys.public_key().to_hex(), FIXTURE_PK_HEX);
}

// ── Random generation ────────────────────────────────────────────────────────

#[test]
fn signer_generates_random_keypair_when_no_secret_provided() {
    let keys = signer::generate();
    assert_eq!(keys.public_key().to_hex().len(), 64);
}

// ── NIP-44 ───────────────────────────────────────────────────────────────────

#[tokio::test]
async fn signer_nip44_encrypt_decrypt_roundtrip() {
    let sender_keys = Keys::generate();
    let recipient_keys = Keys::generate();
    let plaintext = "Hello Encryption!";

    let ciphertext = encrypt_nip44(&sender_keys, &recipient_keys.public_key(), plaintext)
        .await
        .expect("nip44 encrypt");

    assert_ne!(ciphertext, plaintext);

    let decrypted = decrypt_nip44(&recipient_keys, &sender_keys.public_key(), &ciphertext)
        .await
        .expect("nip44 decrypt");

    assert_eq!(decrypted, plaintext);
}

// ── Public key ───────────────────────────────────────────────────────────────

#[test]
fn signer_get_public_key_returns_correct_key() {
    let keys = fixture_keys();
    let expected_pk = PublicKey::parse(FIXTURE_PK_HEX).expect("fixture PK hex parses");
    assert_eq!(keys.public_key(), expected_pk);
}

// ── Signed events ────────────────────────────────────────────────────────────

#[test]
fn signer_signed_event_has_valid_signature() {
    let keys = fixture_keys();
    let event = EventBuilder::new(Kind::TextNote, "Hello Nostr!")
        .sign_with_keys(&keys)
        .expect("sign text note");

    assert_eq!(event.pubkey, keys.public_key());
    event.verify().expect("verify signed event");
}
