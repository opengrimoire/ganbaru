# Sync encryption

Status: Partial. Device encryption keys, key epochs, their envelopes and chain links, and the relay record and asset chunk formats are implemented; the relay, its client, and encrypted backups are planned.

## Primitives

Space encryption uses XChaCha20-Poly1305 from RustCrypto `chacha20poly1305` and RFC 9180 HPKE (DHKEM X25519 with HKDF-SHA256 and ChaCha20-Poly1305) from `hpke`, with `x25519-dalek` for static device encryption keys, because `ring` offers neither XChaCha nor static X25519 secrets. `ring` stays the source of randomness for epoch keys, X25519 secrets, and nonces, and of HKDF for subkeys; only HPKE encapsulation draws from the OS generator that the `hpke` crate's `getrandom` feature provides.

Each crate is pinned exactly with default features off. The chosen `hpke` features exclude ML-KEM, NIST curves, and AES. The review on 2026-10-28 found sources matching upstream commits, no open RustSec advisories, and twelve new lock entries, of which two are RustCrypto duplicates (`cipher`, `inout`). `inout` is held at 0.2.2 because later 0.2 releases only change code behind a feature this tree does not enable.

## Relay formats

A relay record (`GBSR`) carries one stored operation and its writer certificate, sealed under the space key epoch named in its clear header (space, writer, sequence, epoch) with a random nonce; the header is authenticated as associated data after a purpose label, and an opened record whose operation disagrees with its header is refused. A relay asset chunk (`GBSA`) seals one fixed-size chunk of a file under its own subkey with its header (space, asset id, epoch, index, count) as associated data, and the asset id is an HMAC of the plaintext digest under a per-space subkey of epoch 1, so the relay cannot match files across spaces or against known content. A device opens a record or chunk only with an epoch key it opened from its own envelope, so a removed device opens nothing sealed after its removal. The epoch field of the operation header stays zero; the record header carries it.

## Supply-chain risk

`hpke` has a single publisher who also publishes the dalek crates. That risk is contained by the exact pins, RFC 9180 known-answer tests for the chosen suite, an XChaCha20-Poly1305 known-answer test, cargo-audit, and a reviewed source diff plus the two-week release age on every bump of `hpke`, `x25519-dalek`, or `curve25519-dalek`. Device private keys are stored and compared as Ganbaru's own bytes, never as serialized `hpke` keys, because `hpke` 0.15 changes the serialized form of equivalent keys.

`hpke-rs` was considered as a fallback but pulls NIST curves, AES, and the libcrux stack unconditionally.

ChaCha20-Poly1305 is not key-committing. Space keys are random, so this does not matter for sync records; the primitives must not be used with keys derived from low-entropy secrets.
