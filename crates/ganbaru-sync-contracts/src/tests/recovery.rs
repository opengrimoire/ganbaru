use super::fixtures::{RECOVERY_SEED, recovery_seed};
use super::golden::assert_vector;
use crate::crypto::{DEVICE_KEY_ID_DOMAIN, KEY_CHAIN_DOMAIN, SPACE_KEY_INFO};
use crate::ids::hex;
use crate::recovery::{
    KIT_CODE_CHARS, KIT_GROUP_CHARS, KIT_GROUP_SEPARATOR, RECOVERY_ADMIN_INFO, RECOVERY_HPKE_INFO,
};
use crate::{AdminKeyPair, RecoveryError, RecoverySeed, Subkey};
use ring::hmac;

/// HKDF-SHA256 with an empty salt and one 32-byte output block, computed from HMAC directly.
fn reference_hkdf(seed: &[u8], info: &[u8]) -> [u8; 32] {
    let extract = hmac::Key::new(hmac::HMAC_SHA256, &[]);
    let prk = hmac::sign(&extract, seed);
    let expand = hmac::Key::new(hmac::HMAC_SHA256, prk.as_ref());
    let mut block = info.to_vec();
    block.push(1);
    let okm = hmac::sign(&expand, &block);
    let mut output = [0u8; 32];
    output.copy_from_slice(&okm.as_ref()[..32]);
    output
}

#[test]
fn recovery_derivation_and_kit_code_match_their_vectors() {
    let seed = recovery_seed();
    let admin = seed.recovery_admin_key().unwrap();
    assert_vector("recovery-admin-key", &hex(admin.public_key().as_bytes()));
    assert_vector("recovery-kit-code", &hex(seed.kit_code().as_bytes()));
}

#[test]
fn derivation_is_hkdf_sha256_with_a_terminated_label() {
    let seed = recovery_seed();
    let mut info = RECOVERY_ADMIN_INFO.to_vec();
    info.push(0);
    assert_eq!(
        *seed.derive(RECOVERY_ADMIN_INFO),
        reference_hkdf(&RECOVERY_SEED, &info)
    );
    assert_eq!(
        seed.recovery_admin_key().unwrap().public_key(),
        AdminKeyPair::from_seed(&reference_hkdf(&RECOVERY_SEED, &info))
            .unwrap()
            .public_key()
    );
    assert_ne!(
        *seed.derive(b"ganbaru-sync/a"),
        *seed.derive(b"ganbaru-sync/b")
    );
    assert_ne!(
        *seed.derive(RECOVERY_ADMIN_INFO),
        *RecoverySeed::from_bytes([0x67; 32]).derive(RECOVERY_ADMIN_INFO)
    );
}

#[test]
fn kit_codes_are_grouped_and_round_trip() {
    let seed = recovery_seed();
    let code = seed.kit_code();
    let groups: Vec<&str> = code.split(KIT_GROUP_SEPARATOR).collect();
    assert_eq!(groups.len(), KIT_CODE_CHARS.div_ceil(KIT_GROUP_CHARS));
    assert!(groups.iter().all(|group| group.len() == KIT_GROUP_CHARS));
    assert!(
        code.chars()
            .all(|character| character == KIT_GROUP_SEPARATOR
                || "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(character))
    );
    assert_eq!(parsed(&code).unwrap(), *seed.as_bytes());
    for _ in 0..32 {
        let fresh = RecoverySeed::generate().unwrap();
        assert_eq!(parsed(&fresh.kit_code()).unwrap(), *fresh.as_bytes());
    }
}

#[test]
fn kit_code_parsing_tolerates_how_people_type() {
    let seed = recovery_seed();
    let code = seed.kit_code();
    let compact: String = code
        .chars()
        .filter(|character| *character != KIT_GROUP_SEPARATOR)
        .collect();
    let spaced = code.replace(KIT_GROUP_SEPARATOR, " ");
    let lower = code.to_lowercase();
    let confusable: String = code
        .chars()
        .map(|character| match character {
            '0' => 'o',
            '1' => 'L',
            other => other,
        })
        .collect();
    let padded = format!("  {code}\n");
    for typed in [compact, spaced, lower, confusable, padded] {
        assert_eq!(parsed(&typed).unwrap(), *seed.as_bytes(), "{typed}");
    }
    let with_i = code.replacen('1', "I", 1);
    if with_i != code {
        assert_eq!(parsed(&with_i).unwrap(), *seed.as_bytes());
    }
}

#[test]
fn kit_code_parsing_refuses_damaged_codes() {
    let code = recovery_seed().kit_code();
    let with_u = format!("U{}", &code[1..]);
    assert_eq!(parsed(&with_u), Err(RecoveryError::Character));
    assert_eq!(parsed(&format!("{code}!")), Err(RecoveryError::Character));
    assert_eq!(parsed(&code[..code.len() - 1]), Err(RecoveryError::Length));
    assert_eq!(parsed(&format!("{code}0")), Err(RecoveryError::Length));
    assert_eq!(parsed(""), Err(RecoveryError::Length));

    let alphabet = "0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let characters: Vec<char> = code.chars().collect();
    for (index, original) in characters.iter().enumerate() {
        if *original == KIT_GROUP_SEPARATOR {
            continue;
        }
        for replacement in alphabet.chars().filter(|candidate| candidate != original) {
            let mut damaged = characters.clone();
            damaged[index] = replacement;
            let damaged: String = damaged.into_iter().collect();
            assert_eq!(parsed(&damaged), Err(RecoveryError::Checksum), "{damaged}");
        }
    }
    let mut swapped: Vec<char> = code.chars().collect();
    let first = swapped
        .iter()
        .position(|c| *c != KIT_GROUP_SEPARATOR)
        .unwrap();
    let second = (first + 1..swapped.len())
        .find(|index| swapped[*index] != KIT_GROUP_SEPARATOR && swapped[*index] != swapped[first])
        .unwrap();
    swapped.swap(first, second);
    let swapped: String = swapped.into_iter().collect();
    assert_eq!(parsed(&swapped), Err(RecoveryError::Checksum));
}

#[test]
fn recovery_seeds_never_print_their_bytes() {
    let seed = recovery_seed();
    let printed = format!("{seed:?}");
    assert!(!printed.contains(&hex(seed.as_bytes())));
    assert!(!printed.contains("66"));
}

/// The seed bytes a typed code parses to.
fn parsed(code: &str) -> Result<[u8; 32], RecoveryError> {
    RecoverySeed::parse_kit_code(code).map(|seed| *seed.as_bytes())
}

#[test]
fn the_recovery_encryption_key_derives_with_its_own_label() {
    let seed = recovery_seed();
    let encryption = seed.recovery_encryption_key().unwrap();
    assert_vector(
        "recovery-encryption-key",
        &hex(encryption.public_key().as_bytes()),
    );
    let mut info = RECOVERY_HPKE_INFO.to_vec();
    info.push(0);
    assert_eq!(
        *encryption.secret_bytes(),
        reference_hkdf(&RECOVERY_SEED, &info)
    );
    for label in [
        SPACE_KEY_INFO,
        KEY_CHAIN_DOMAIN,
        DEVICE_KEY_ID_DOMAIN,
        RECOVERY_ADMIN_INFO,
    ]
    .into_iter()
    .chain(Subkey::ALL.iter().map(|purpose| purpose.info()))
    {
        assert_ne!(RECOVERY_HPKE_INFO, label);
    }
    let keys = seed.public_keys().unwrap();
    assert_eq!(keys.admin, seed.recovery_admin_key().unwrap().public_key());
    assert_eq!(keys.encryption, encryption.public_key());
    assert_ne!(
        RecoverySeed::from_bytes([0x67; 32])
            .public_keys()
            .unwrap()
            .encryption,
        keys.encryption
    );
}

#[test]
fn seeds_compare_by_value() {
    let seed = recovery_seed();
    assert!(seed.ct_eq(&RecoverySeed::from_bytes(RECOVERY_SEED)));
    assert!(!seed.ct_eq(&RecoverySeed::from_bytes([0x67; 32])));
}

#[test]
fn only_the_last_group_confirms_a_saved_kit() {
    let seed = recovery_seed();
    let code = seed.kit_code();
    let last = code.rsplit(KIT_GROUP_SEPARATOR).next().unwrap();
    assert!(seed.last_group_matches(last));
    assert!(seed.last_group_matches(&format!(" {} ", last.to_lowercase())));
    let first = code.split(KIT_GROUP_SEPARATOR).next().unwrap();
    if first != last {
        assert!(!seed.last_group_matches(first));
    }
    assert!(!seed.last_group_matches(""));
    assert!(!seed.last_group_matches(&last[..last.len() - 1]));
    assert!(!seed.last_group_matches(&format!("{last}0")));
    assert!(!seed.last_group_matches(&code));
    assert!(!seed.last_group_matches("!!!!"));
    let other = RecoverySeed::from_bytes([0x67; 32]).kit_code();
    let other_last = other.rsplit(KIT_GROUP_SEPARATOR).next().unwrap();
    if other_last != last {
        assert!(!seed.last_group_matches(other_last));
    }
}
