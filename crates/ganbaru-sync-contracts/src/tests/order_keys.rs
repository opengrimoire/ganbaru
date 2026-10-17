use super::fixtures::XorShift;
use crate::bounds::MAX_ORDER_KEY_BYTES;
use crate::order_key::RANK_KEY_CAPACITY;
use crate::{OrderKey, OrderKeyError};

fn key(text: &str) -> OrderKey {
    OrderKey::parse(text).unwrap()
}

fn smallest_integer() -> String {
    format!("A{}", "0".repeat(26))
}

#[test]
fn parse_accepts_the_grammar_and_rejects_everything_else() {
    for valid in [
        "a0", "a1", "aV", "b00", "Zz", "a0V", "c000", "czzz", "a0001",
    ] {
        assert!(OrderKey::parse(valid).is_ok(), "{valid} should be valid");
    }
    let smallest = smallest_integer();
    let too_long = format!("a0{}", "1".repeat(MAX_ORDER_KEY_BYTES - 1));
    for invalid in [
        "",
        "a",
        "b0",
        "a00",
        "a0V0",
        "a0 ",
        "a0-",
        "!0",
        "0a",
        smallest.as_str(),
        too_long.as_str(),
    ] {
        assert_eq!(
            OrderKey::parse(invalid),
            Err(OrderKeyError::Invalid),
            "{invalid:?} should be invalid"
        );
    }
    let smallest_with_fraction = format!("{smallest}1");
    assert!(OrderKey::parse(&smallest_with_fraction).is_ok());
}

#[test]
fn first_before_after_and_between_follow_the_reference_algorithm() {
    assert_eq!(OrderKey::first().as_str(), "a0");
    assert_eq!(OrderKey::between(None, None).unwrap().as_str(), "a0");
    assert_eq!(key("a0").key_after().unwrap().as_str(), "a1");
    assert_eq!(key("a0").key_before().unwrap().as_str(), "Zz");
    assert_eq!(key("az").key_after().unwrap().as_str(), "b00");
    assert_eq!(key("b00").key_before().unwrap().as_str(), "az");
    assert_eq!(key("a0V").key_before().unwrap().as_str(), "a0");
    assert_eq!(
        OrderKey::between(Some(&key("a0")), Some(&key("a1")))
            .unwrap()
            .as_str(),
        "a0V"
    );
    assert_eq!(
        OrderKey::between(Some(&key("a0")), Some(&key("a2")))
            .unwrap()
            .as_str(),
        "a1"
    );
    assert_eq!(
        OrderKey::between(Some(&key("a1")), Some(&key("a1V")))
            .unwrap()
            .as_str(),
        "a1G"
    );
    assert_eq!(
        OrderKey::between(Some(&key("a0V")), Some(&key("a0W")))
            .unwrap()
            .as_str(),
        "a0VV"
    );
}

#[test]
fn between_rejects_unordered_bounds() {
    assert_eq!(
        OrderKey::between(Some(&key("a1")), Some(&key("a1"))),
        Err(OrderKeyError::Unordered)
    );
    assert_eq!(
        OrderKey::between(Some(&key("a2")), Some(&key("a1"))),
        Err(OrderKeyError::Unordered)
    );
}

#[test]
fn keys_near_the_smallest_integer_stay_valid() {
    let smallest = smallest_integer();
    let one_above = format!("A{}1", "0".repeat(25));
    let before = key(&one_above).key_before().unwrap();
    assert!(before < key(&one_above));
    assert!(before.as_str().starts_with(&smallest));

    let with_fraction = key(&format!("{smallest}1"));
    let before = with_fraction.key_before().unwrap();
    assert!(before < with_fraction);
    assert!(OrderKey::parse(before.as_str()).is_ok());
}

#[test]
fn keys_after_the_largest_integer_use_fractions() {
    let largest = key(&format!("z{}", "z".repeat(26)));
    let after = largest.key_after().unwrap();
    assert!(after > largest);
    assert_eq!(after.as_str().len(), largest.as_str().len() + 1);
}

#[test]
fn descending_and_ascending_chains_stay_ordered() {
    let mut current = OrderKey::first();
    for _ in 0..5_000 {
        let next = current.key_before().unwrap();
        assert!(next < current);
        current = next;
    }
    let mut current = OrderKey::first();
    for _ in 0..5_000 {
        let next = current.key_after().unwrap();
        assert!(next > current);
        current = next;
    }
}

#[test]
fn random_insertions_stay_strictly_between_neighbors() {
    let mut random = XorShift::new(0x0de7_0001);
    let mut keys = vec![OrderKey::rank(10).unwrap(), OrderKey::rank(11).unwrap()];
    for _ in 0..3_000 {
        let position = random.below(keys.len() + 1);
        let low = position.checked_sub(1).map(|index| &keys[index]);
        let high = keys.get(position);
        let created = match OrderKey::between(low, high) {
            Ok(created) => created,
            Err(OrderKeyError::Exhausted) => continue,
            Err(error) => panic!("unexpected order key error {error}"),
        };
        assert!(low.is_none_or(|low| *low < created));
        assert!(high.is_none_or(|high| created < *high));
        assert!(created.as_str().len() <= MAX_ORDER_KEY_BYTES);
        assert_eq!(OrderKey::parse(created.as_str()).unwrap(), created);
        keys.insert(position, created);
    }
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn repeated_insertion_at_one_point_exhausts_without_panicking() {
    let low = OrderKey::first();
    let mut high = low.key_after().unwrap();
    let mut created = 0;
    loop {
        match OrderKey::between(Some(&low), Some(&high)) {
            Ok(key) => {
                assert!(low < key && key < high);
                assert!(key.as_str().len() <= MAX_ORDER_KEY_BYTES);
                high = key;
                created += 1;
            }
            Err(error) => {
                assert_eq!(error, OrderKeyError::Exhausted);
                break;
            }
        }
    }
    assert!(created > 100, "only {created} keys fit before exhaustion");
}

#[test]
fn rank_keys_are_valid_ordered_and_bounded() {
    assert_eq!(OrderKey::rank(0).unwrap().as_str(), "c000");
    assert_eq!(OrderKey::rank(1).unwrap().as_str(), "c001");
    assert_eq!(OrderKey::rank(61).unwrap().as_str(), "c00z");
    assert_eq!(OrderKey::rank(62).unwrap().as_str(), "c010");
    assert_eq!(
        OrderKey::rank(RANK_KEY_CAPACITY - 1).unwrap().as_str(),
        "czzz"
    );
    assert_eq!(
        OrderKey::rank(RANK_KEY_CAPACITY),
        Err(OrderKeyError::Exhausted)
    );
    let mut previous = OrderKey::rank(0).unwrap();
    for position in 1..RANK_KEY_CAPACITY {
        let current = OrderKey::rank(position).unwrap();
        assert!(previous < current);
        previous = current;
    }
    let between = OrderKey::between(
        Some(&OrderKey::rank(5).unwrap()),
        Some(&OrderKey::rank(6).unwrap()),
    )
    .unwrap();
    assert!(OrderKey::rank(5).unwrap() < between && between < OrderKey::rank(6).unwrap());
    assert!(OrderKey::rank(0).unwrap().key_before().unwrap() < OrderKey::rank(0).unwrap());
}
