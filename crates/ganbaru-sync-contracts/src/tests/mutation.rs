use super::fixtures::{
    XorShift, certificate_for, fully_verifies, golden_authorization_log, golden_cause,
    golden_chain, golden_charter, golden_entries, golden_key_entries, other_writer_id, person,
    proposal, writer_key,
};
use crate::{
    AuthorizationRecord, Envelope, SignedCertificate, SignedCharter, SignedProposal, VersionVector,
};

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
    for (_, bytes) in golden_chain().into_iter().chain([golden_cause()]) {
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
    for (_, bytes) in golden_chain().into_iter().chain([golden_cause()]) {
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
        let _ = SignedCharter::decode(&bytes);
        let _ = SignedProposal::decode(&bytes);
        let _ = AuthorizationRecord::decode(&bytes);
        let _ = VersionVector::decode(&bytes);
    }
}

/// Decodes and verifies an authorization artifact the way a replica will.
type Verifier = fn(&[u8]) -> bool;

fn charter_verifies(bytes: &[u8]) -> bool {
    SignedCharter::decode(bytes).is_ok_and(|charter| charter.verify(&person().public_key()).is_ok())
}

fn certificate_verifies(bytes: &[u8]) -> bool {
    SignedCertificate::decode(bytes).is_ok()
}

fn proposal_verifies(bytes: &[u8]) -> bool {
    SignedProposal::decode(bytes).is_ok()
}

fn authorization_vectors() -> Vec<(Vec<u8>, Verifier)> {
    let mut vectors: Vec<(Vec<u8>, Verifier)> = vec![
        (golden_charter().encode(), charter_verifies),
        (
            certificate_for(&writer_key().public_key(), Some(other_writer_id())).encode(),
            certificate_verifies,
        ),
    ];
    for (_, entry, _) in golden_entries().into_iter().chain(golden_key_entries()) {
        vectors.push((proposal(entry, 0).encode(), proposal_verifies));
    }
    vectors
}

#[test]
fn every_single_bit_flip_of_an_authorization_operation_fails_verification() {
    for (name, _, bytes, key) in golden_authorization_log() {
        assert!(fully_verifies(&bytes, &key), "{name}");
        for index in 0..bytes.len() {
            for bit in 0..8 {
                let mut mutated = bytes.clone();
                mutated[index] ^= 1 << bit;
                assert!(
                    !fully_verifies(&mutated, &key),
                    "{name}: flipping bit {bit} of byte {index} still verified"
                );
            }
        }
    }
}

#[test]
fn every_single_bit_flip_of_a_charter_certificate_or_proposal_fails_verification() {
    for (bytes, verifies) in authorization_vectors() {
        assert!(verifies(&bytes));
        for index in 0..bytes.len() {
            for bit in 0..8 {
                let mut mutated = bytes.clone();
                mutated[index] ^= 1 << bit;
                assert!(
                    !verifies(&mutated),
                    "flipping bit {bit} of byte {index} still verified"
                );
            }
        }
    }
}

#[test]
fn mutated_authorization_artifacts_never_panic_and_never_verify() {
    let mut random = XorShift::new(0x5eed_0003);
    let operations = golden_authorization_log()
        .into_iter()
        .map(|(_, _, bytes, key)| (bytes, Some(key)));
    let artifacts = authorization_vectors()
        .into_iter()
        .map(|(bytes, _)| (bytes, None));
    let verifiers: Vec<Verifier> = authorization_vectors()
        .into_iter()
        .map(|(_, verifies)| verifies)
        .collect();
    for (index, (bytes, key)) in operations.chain(artifacts).enumerate() {
        let mut checked = 0;
        while checked < MUTATIONS_PER_VECTOR / 3 {
            let mutated = mutate(&bytes, &mut random);
            if mutated == bytes {
                continue;
            }
            let verified = match &key {
                Some(key) => fully_verifies(&mutated, key),
                None => verifiers.iter().any(|verifies| verifies(&mutated)),
            };
            assert!(!verified, "vector {index} verified after mutation");
            checked += 1;
        }
    }
}
