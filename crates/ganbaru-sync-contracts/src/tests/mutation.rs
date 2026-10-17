use super::fixtures::{XorShift, fully_verifies, golden_chain, writer_key};
use crate::{Envelope, SignedCertificate, VersionVector};

const MUTATIONS_PER_VECTOR: usize = 3_000;
const RANDOM_INPUTS: usize = 2_000;

fn mutate(bytes: &[u8], random: &mut XorShift) -> Vec<u8> {
    let mut mutated = bytes.to_vec();
    match random.below(6) {
        0 => {
            let index = random.below(mutated.len());
            mutated[index] ^= 1 << random.below(8);
        }
        1 => {
            for _ in 0..=random.below(8) {
                let index = random.below(mutated.len());
                mutated[index] ^= 1 << random.below(8);
            }
        }
        2 => mutated.truncate(random.below(mutated.len())),
        3 => {
            for _ in 0..=random.below(64) {
                mutated.push(random.byte());
            }
        }
        4 => {
            let index = random.below(mutated.len() + 1);
            mutated.insert(index, random.byte());
        }
        _ => {
            let start = random.below(mutated.len());
            let end = (start + 1 + random.below(16)).min(mutated.len());
            for byte in &mut mutated[start..end] {
                *byte = random.byte();
            }
        }
    }
    mutated
}

#[test]
fn mutated_operations_never_panic_and_never_verify() {
    let public_key = writer_key().public_key();
    let mut random = XorShift::new(0x5eed_0001);
    for (_, bytes) in golden_chain() {
        assert!(fully_verifies(&bytes, &public_key));
        let mut checked = 0;
        while checked < MUTATIONS_PER_VECTOR {
            let mutated = mutate(&bytes, &mut random);
            if mutated == bytes {
                continue;
            }
            assert!(
                !fully_verifies(&mutated, &public_key),
                "a mutated operation verified: {}",
                crate::ids::hex(&mutated)
            );
            checked += 1;
        }
    }
}

#[test]
fn every_single_bit_flip_of_every_vector_fails_verification() {
    let public_key = writer_key().public_key();
    for (_, bytes) in golden_chain() {
        for index in 0..bytes.len() {
            for bit in 0..8 {
                let mut mutated = bytes.clone();
                mutated[index] ^= 1 << bit;
                assert!(
                    !fully_verifies(&mutated, &public_key),
                    "flipping bit {bit} of byte {index} still verified"
                );
            }
        }
    }
}

#[test]
fn random_inputs_never_panic_the_decoders() {
    let mut random = XorShift::new(0x5eed_0002);
    let golden = golden_chain();
    for round in 0..RANDOM_INPUTS {
        let length = random.below(512);
        let mut bytes: Vec<u8> = (0..length).map(|_| random.byte()).collect();
        if round % 2 == 0 && bytes.len() >= 4 {
            // Start like a real operation so decoding reaches deeper fields.
            let prefix = &golden[round % golden.len()].1;
            let keep = random.below(prefix.len().min(bytes.len()));
            bytes[..keep].copy_from_slice(&prefix[..keep]);
        }
        if let Ok(envelope) = Envelope::decode(&bytes) {
            let _ = envelope.operation();
            let _ = envelope.genesis();
        }
        let _ = SignedCertificate::decode(&bytes);
        let _ = VersionVector::decode(&bytes);
    }
}
