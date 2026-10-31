use std::collections::HashMap;

use ganbaru_sync::{ContestInstance, ContestObjection, ContestYield, DeviceNames};
use ganbaru_sync_contracts::WriterId;
use serde_json::json;

use super::*;

const OWN: &str = "device-own";
const PAIRED: &str = "device-paired";
const ENROLLED_ONLY: &str = "device-enrolled";

fn names() -> Names {
    Names {
        paired: DeviceNames::new(
            OWN,
            HashMap::from([(PAIRED.to_string(), "Paired laptop".to_string())]),
        ),
        enrollment_labels: HashMap::from([
            (PAIRED.to_string(), "Enrolled laptop".to_string()),
            (ENROLLED_ONLY.to_string(), "Enrolled phone".to_string()),
        ]),
    }
}

fn entry(byte: u8, revision: u64) -> ContestEntry {
    ContestEntry {
        hash: [byte; 32],
        revision,
    }
}

fn view() -> ContestsView {
    ContestsView {
        contests: vec![ContestView {
            instance: ContestInstance {
                root_hash: [1; 32],
                epoch: 3,
                anchor_device_id: ENROLLED_ONLY.to_string(),
            },
            frozen_at: Some(entry(2, 9)),
            cause: ContestCause::Contested,
            candidates: vec![
                ContestCandidate {
                    hash: [3; 32],
                    parent: entry(2, 9),
                    kind: CandidateKind::Forced,
                    target_device_id: OWN.to_string(),
                    signer_device_id: Some(OWN.to_string()),
                    target_enrolled_revision: Some(2),
                    target_enroller_device_id: None,
                    withdrawn_in_favor_of: None,
                    local: true,
                },
                ContestCandidate {
                    hash: [4; 32],
                    parent: entry(2, 9),
                    kind: CandidateKind::Kit,
                    target_device_id: PAIRED.to_string(),
                    signer_device_id: None,
                    target_enrolled_revision: Some(5),
                    target_enroller_device_id: Some(OWN.to_string()),
                    withdrawn_in_favor_of: Some([3; 32]),
                    local: false,
                },
            ],
            objections: vec![ContestObjection {
                hash: [5; 32],
                against: [4; 32],
                objector_device_id: Some(ENROLLED_ONLY.to_string()),
                withdrawn: false,
            }],
            yields: vec![ContestYield {
                hash: [6; 32],
                author_device_id: None,
                to: [3; 32],
            }],
            held_device_ids: vec![PAIRED.to_string()],
            local_role: ContestRole::Contestant { candidate: [3; 32] },
            actions: vec![
                ContestAction::YieldTo { candidate: [4; 32] },
                ContestAction::UseKit {
                    points: vec![
                        KitPoint {
                            entry: entry(2, 9),
                            epoch: 3,
                            place: KitPointPlace::Head,
                        },
                        KitPoint {
                            entry: entry(4, 10),
                            epoch: 4,
                            place: KitPointPlace::Candidate {
                                target_device_id: PAIRED.to_string(),
                            },
                        },
                    ],
                },
            ],
        }],
        losing_removals: vec![LosingRemoval {
            device_id: PAIRED.to_string(),
            removed_by_device_id: Some(ENROLLED_ONLY.to_string()),
            revision: 11,
        }],
    }
}

#[test]
fn devices_take_the_pairing_label_then_the_enrollment_label() {
    let names = names();

    assert_eq!(
        names.describe(PAIRED).device_label.as_deref(),
        Some("Paired laptop")
    );
    assert_eq!(
        names.describe(ENROLLED_ONLY).device_label.as_deref(),
        Some("Enrolled phone")
    );
    assert_eq!(names.describe("device-unknown").device_label, None);
    assert!(names.describe(OWN).own_device);
}

#[test]
fn a_contest_serializes_with_hex_hashes_tagged_roles_and_actions() {
    let dto = contests_dto(&view(), None, &[], &names(), true);
    let value = serde_json::to_value(&dto).expect("serialize contests");
    let contest = &value["contests"][0];
    let own_hash = "03".repeat(32);
    let other_hash = "04".repeat(32);

    assert_eq!(contest["rootHash"], json!("01".repeat(32)));
    assert_eq!(contest["anchor"]["deviceLabel"], json!("Enrolled phone"));
    assert_eq!(
        contest["frozenAt"],
        json!({ "hash": "02".repeat(32), "revision": 9 })
    );
    assert_eq!(contest["cause"], json!("contested"));
    assert_eq!(contest["candidates"][0]["kind"], json!("forced"));
    assert_eq!(contest["candidates"][1]["signer"], json!(null));
    assert_eq!(
        contest["candidates"][1]["withdrawnInFavorOf"],
        json!(own_hash)
    );
    assert_eq!(
        contest["candidates"][1]["targetEnroller"]["ownDevice"],
        json!(true)
    );
    assert_eq!(contest["objections"][0]["against"], json!(other_hash));
    assert_eq!(contest["yields"][0]["author"], json!(null));
    assert_eq!(
        contest["localRole"],
        json!({ "kind": "contestant", "candidate": own_hash })
    );
    assert_eq!(
        contest["actions"][0],
        json!({ "kind": "yieldTo", "candidate": other_hash })
    );
    assert_eq!(contest["actions"][1]["kind"], json!("useKit"));
    assert_eq!(
        contest["actions"][1]["points"][0]["place"],
        json!({ "kind": "head" })
    );
    assert_eq!(
        contest["actions"][1]["points"][1]["place"]["target"]["deviceLabel"],
        json!("Paired laptop")
    );
    assert_eq!(
        value["losingRemovals"][0]["removedBy"]["deviceId"],
        json!(ENROLLED_ONLY)
    );
    assert_eq!(value["storage"], json!({ "backstop": null, "alerts": [] }));
}

#[test]
fn without_the_kit_the_kit_action_is_left_out_and_the_others_stay() {
    let dto = contests_dto(&view(), None, &[], &names(), false);

    assert_eq!(
        dto.contests[0].actions,
        vec![ActionDto::YieldTo {
            candidate: "04".repeat(32)
        }]
    );
}

#[test]
fn storage_reports_the_backstop_and_each_growth_alert() {
    let writer = WriterId::from_bytes([9; 16]);
    let alerts = [
        GrowthAlert::Device {
            device_id: PAIRED.to_string(),
            bytes: 70,
        },
        GrowthAlert::Total { bytes: 900 },
        GrowthAlert::ForkedWriter {
            writer,
            device_id: None,
        },
    ];
    let dto = contests_dto(
        &ContestsView {
            contests: Vec::new(),
            losing_removals: Vec::new(),
        },
        Some(BackstopState::Stopped),
        &alerts,
        &names(),
        true,
    );
    let value = serde_json::to_value(&dto.storage).expect("serialize storage");

    assert_eq!(value["backstop"], json!("stopped"));
    assert_eq!(value["alerts"][0]["kind"], json!("device"));
    assert_eq!(
        value["alerts"][0]["device"]["deviceLabel"],
        json!("Paired laptop")
    );
    assert_eq!(value["alerts"][0]["bytes"], json!(70));
    assert_eq!(value["alerts"][1], json!({ "kind": "total", "bytes": 900 }));
    assert_eq!(
        value["alerts"][2],
        json!({ "kind": "forkedWriter", "writer": writer.to_hex(), "device": null })
    );
}
