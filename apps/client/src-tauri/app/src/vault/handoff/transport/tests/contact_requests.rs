//! Contact requests over the coordinator listener: forwarding, uniform answer timing, rejection,
//! and the per-source budget that keeps a flood from delaying coordinator requests.

use super::*;

#[tokio::test]
async fn unauthenticated_contact_request_is_forwarded_to_the_contact_worker() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let signature = key.sign(&ganbaru_contacts::request_signature_payload(
        &recipient_nonce,
        "contact-one",
    ));
    let expected_card = card.clone();
    let coordinator = tokio::spawn(async move {
        let job = contacts.recv().await.expect("contact request");
        match job.operation {
            ContactOperation::Request {
                recipient_card_nonce,
                requester_card,
                request_id,
                ..
            } => {
                assert_eq!(
                    recipient_card_nonce,
                    ganbaru_contacts::encode_nonce(&recipient_nonce)
                );
                assert_eq!(requester_card, expected_card);
                assert_eq!(request_id, "contact-one");
            }
            operation => panic!("unexpected contact operation: {operation:?}"),
        }
        job.response
            .send(Ok(PeerReply::Received {
                request_id: "contact-one".to_string(),
            }))
            .expect("send contact response");
    });

    let response = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        ControlMessage::ContactRequest {
            protocol_version: PROTOCOL_VERSION,
            recipient_card_nonce: ganbaru_contacts::encode_nonce(&recipient_nonce),
            requester_card: card,
            request_id: "contact-one".to_string(),
            signature: base64url(&signature),
        },
    )
    .await
    .expect("contact request exchange");
    assert!(matches!(
        response,
        ControlMessage::ContactRequestReceived { request_id } if request_id == "contact-one"
    ));
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn contact_request_answers_wait_for_the_same_minimum_time_whatever_the_outcome() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, card) = sample_contact_card();
    let recipient_nonce = [7u8; ganbaru_contacts::CARD_NONCE_BYTES];
    let replies = [
        PeerReply::Received {
            request_id: "contact-one".to_string(),
        },
        PeerReply::Rejected {
            code: "invalid_request",
            message: "the contact request is invalid",
            retryable: false,
        },
    ];
    let coordinator = tokio::spawn(async move {
        for reply in replies {
            let job = contacts.recv().await.expect("contact request");
            job.response.send(Ok(reply)).expect("send contact response");
        }
    });

    for _ in 0..2 {
        let signature = key.sign(&ganbaru_contacts::request_signature_payload(
            &recipient_nonce,
            "contact-one",
        ));
        let started = std::time::Instant::now();
        let _ = unauthenticated_exchange(
            &pair.invitation.endpoint,
            &pair.invitation.coordinator_fingerprint,
            ControlMessage::ContactRequest {
                protocol_version: PROTOCOL_VERSION,
                recipient_card_nonce: ganbaru_contacts::encode_nonce(&recipient_nonce),
                requester_card: card.clone(),
                request_id: "contact-one".to_string(),
                signature: base64url(&signature),
            },
        )
        .await;
        assert!(
            started.elapsed()
                >= crate::vault::handoff::contact_requests::MIN_CONTACT_REQUEST_RESPONSE
        );
    }
    coordinator.await.expect("coordinator task");
}

#[tokio::test]
async fn contact_rejection_reaches_the_requester_as_a_protocol_error() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let (key, _) = sample_contact_card();
    let issued_at_ms = unix_time_ms();
    let signature = key.sign(&ganbaru_contacts::status_signature_payload(
        "contact-one",
        issued_at_ms,
    ));
    let coordinator = tokio::spawn(async move {
        let job = contacts.recv().await.expect("status request");
        assert!(matches!(
            job.operation,
            ContactOperation::Status { ref request_id, .. } if request_id == "contact-one"
        ));
        job.response
            .send(Ok(PeerReply::Rejected {
                code: "card_revoked",
                message: "the recipient regenerated their contact card",
                retryable: false,
            }))
            .expect("send rejection");
    });

    let response = unauthenticated_exchange(
        &pair.invitation.endpoint,
        &pair.invitation.coordinator_fingerprint,
        ControlMessage::ContactRequestStatus {
            protocol_version: PROTOCOL_VERSION,
            request_id: "contact-one".to_string(),
            requester_public_key: key.public_key().to_text(),
            issued_at_ms,
            signature: base64url(&signature),
        },
    )
    .await
    .expect("status exchange");
    assert!(matches!(
        response,
        ControlMessage::Error { code, retryable: false, .. } if code == "card_revoked"
    ));
    coordinator.await.expect("coordinator task");
}

fn is_busy(message: &ControlMessage) -> bool {
    matches!(
        message,
        ControlMessage::Error { code, retryable: true, .. } if code == RECIPIENT_BUSY_CODE
    )
}

#[tokio::test]
async fn contact_request_flood_does_not_delay_coordinator_requests() {
    use super::super::super::coordinator::{CoordinatorOperation, CoordinatorResponse};

    let (pair, mut requests, mut contacts) = LocalPair::start_with_services().await;
    pair.enroll().await;
    let slow_worker = tokio::spawn(async move {
        while let Some(job) = contacts.recv().await {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let request_id = match job.operation {
                ContactOperation::Request { request_id, .. } => request_id,
                operation => panic!("unexpected contact operation: {operation:?}"),
            };
            let _ = job.response.send(Ok(PeerReply::Received { request_id }));
        }
    });
    let coordinator = tokio::spawn(async move {
        while let Some(request) = requests.recv().await {
            let CoordinatorOperation::Prepare { .. } = request.operation else {
                panic!("unexpected coordinator operation");
            };
            let _ = request.response.send(Ok(CoordinatorResponse::Rejected {
                code: "copy_declined".to_string(),
                message: "the coordinator answered".to_string(),
                retryable: false,
            }));
        }
    });

    // Stays under the per-source pending connection cap so the poll can still connect.
    let flood: Vec<_> = (0..6)
        .map(|index| {
            let endpoint = pair.invitation.endpoint.clone();
            let fingerprint = pair.invitation.coordinator_fingerprint.clone();
            tokio::spawn(async move {
                unauthenticated_exchange(
                    &endpoint,
                    &fingerprint,
                    contact_request_message(&format!("flood-{index}")),
                )
                .await
            })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(50)).await;

    let answer = tokio::time::timeout(Duration::from_secs(2), prepare_copy(&pair.phone))
        .await
        .expect("coordinator requests stay responsive during a contact flood")
        .expect_err("the test coordinator declines the copy");
    assert_eq!(answer, "the coordinator answered");

    for exchange in flood {
        let response = exchange.await.expect("flood task").expect("flood exchange");
        assert!(
            matches!(response, ControlMessage::ContactRequestReceived { .. }) || is_busy(&response),
            "unexpected flood response: {response:?}"
        );
    }
    slow_worker.abort();
    coordinator.abort();
}

#[tokio::test]
async fn contact_requests_past_the_source_budget_get_a_busy_answer() {
    let (pair, _requests, mut contacts) = LocalPair::start_with_services().await;
    let worker = tokio::spawn(async move {
        while let Some(job) = contacts.recv().await {
            let request_id = match job.operation {
                ContactOperation::Request { request_id, .. } => request_id,
                operation => panic!("unexpected contact operation: {operation:?}"),
            };
            let _ = job.response.send(Ok(PeerReply::Received { request_id }));
        }
    });

    let mut received = 0;
    let mut busy = 0;
    for index in 0..24 {
        let response = unauthenticated_exchange(
            &pair.invitation.endpoint,
            &pair.invitation.coordinator_fingerprint,
            contact_request_message(&format!("burst-{index}")),
        )
        .await
        .expect("burst exchange");
        if is_busy(&response) {
            busy += 1;
        } else {
            assert!(
                matches!(response, ControlMessage::ContactRequestReceived { .. }),
                "unexpected burst response: {response:?}"
            );
            received += 1;
        }
    }
    assert!(
        received >= server::limits::CONTACT_MESSAGE_BURST as usize,
        "the burst allowance is honored"
    );
    assert!(busy > 0, "messages past the budget are refused as busy");
    worker.abort();
}
