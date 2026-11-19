//! Coordinator dialing: candidate order, refused endpoints, discovery retries, dial failure
//! reports, and the remembered coordinator contact.

use super::*;

fn pin_with(
    endpoint: &str,
    last_endpoint: Option<&str>,
    reported_endpoints: &[&str],
) -> CoordinatorPin {
    CoordinatorPin {
        device_id: "device-desktop".to_string(),
        device_label: None,
        endpoint: endpoint.to_string(),
        vault_id: "vault-1".to_string(),
        certificate_fingerprint: "a".repeat(64),
        last_endpoint: last_endpoint.map(str::to_string),
        reported_endpoints: reported_endpoints
            .iter()
            .map(|endpoint| endpoint.to_string())
            .collect(),
        charter: None,
    }
}

fn socket_address(endpoint: &str) -> SocketAddr {
    endpoint.parse().expect("endpoint")
}

fn linked_pin(manager: &PairingManager) -> CoordinatorPin {
    manager
        .coordinator_pin()
        .expect("read coordinator pin")
        .expect("linked device")
}

#[test]
fn coordinator_candidates_put_the_last_working_endpoint_first_and_the_invitation_last() {
    let pin = pin_with(
        "192.168.1.20:43821",
        Some("10.0.0.5:43821"),
        &["192.168.1.20:43821", "[fd00::20]:43821", "10.0.0.5:43821"],
    );
    let (candidates, failures) =
        coordinator_candidates(&pin, &[socket_address("172.16.0.9:43821")], |_| Ok(()));
    assert_eq!(
        candidates,
        vec![
            socket_address("10.0.0.5:43821"),
            socket_address("192.168.1.20:43821"),
            socket_address("[fd00::20]:43821"),
            socket_address("172.16.0.9:43821"),
        ]
    );
    assert!(failures.is_empty(), "{failures:?}");
}

#[test]
fn coordinator_candidates_skip_refused_endpoints_and_stop_at_the_cap() {
    let pin = pin_with("192.168.1.20:43821", None, &["10.0.0.5:43821"]);
    let refused = socket_address("10.0.0.5:43821");
    let (candidates, failures) = coordinator_candidates(&pin, &[], |endpoint| {
        if endpoint == refused {
            Err("refuse device connection: test".to_string())
        } else {
            Ok(())
        }
    });
    assert_eq!(candidates, vec![socket_address("192.168.1.20:43821")]);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].endpoint, "10.0.0.5:43821");

    let discovered: Vec<SocketAddr> = (1..=20)
        .map(|host| SocketAddr::from(([10, 0, 1, host], 43_821)))
        .collect();
    let (candidates, _) = coordinator_candidates(&pin, &discovered, |_| Ok(()));
    assert_eq!(candidates.len(), MAX_COORDINATOR_CANDIDATES);
    assert_eq!(candidates[0], refused);
    assert!(
        !candidates.contains(&socket_address("192.168.1.20:43821")),
        "the invitation endpoint falls past the cap here"
    );
}

#[test]
fn discovery_retries_dial_only_endpoints_not_tried_and_authorized() {
    let tried = [
        socket_address("192.168.1.20:43821"),
        socket_address("10.0.0.5:43821"),
    ];
    let refused = socket_address("[fd00::7]:43821");
    let discovered = [
        socket_address("10.0.0.5:43821"),
        socket_address("192.168.1.30:43821"),
        refused,
        socket_address("192.168.1.30:43821"),
        socket_address("172.16.0.9:43821"),
    ];
    let mut failures = Vec::new();
    let untried = untried_discovered_endpoints(
        &discovered,
        &tried,
        |endpoint| {
            if endpoint == refused {
                Err("refuse device connection: test".to_string())
            } else {
                Ok(())
            }
        },
        &mut failures,
    );
    assert_eq!(
        untried,
        vec![
            socket_address("192.168.1.30:43821"),
            socket_address("172.16.0.9:43821"),
        ]
    );
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].endpoint, refused.to_string());
}

#[test]
fn a_single_dial_failure_keeps_its_message_and_several_are_summarized() {
    let failure = |endpoint: &str, error: &str| DialFailure::new(endpoint, error);
    assert_eq!(
        coordinator_dial_error(vec![failure("10.0.0.5:1", "refuse device connection: x")]),
        "refuse device connection: x"
    );
    let summary = coordinator_dial_error(vec![
        failure("10.0.0.5:1", "coordinator connection timed out"),
        failure("[fd00::5]:1", "connect to coordinator: refused"),
    ]);
    assert!(
        summary.starts_with("could not connect to the coordinator at any known endpoint"),
        "{summary}"
    );
    assert!(summary.contains("10.0.0.5:1: coordinator connection timed out"));
    assert!(summary.contains("[fd00::5]:1: connect to coordinator: refused"));
}

#[tokio::test]
async fn a_coordinator_dial_passes_foreign_and_silent_endpoints_and_remembers_the_one_that_answered()
 {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let foreign = LocalPair::start().await;
    let silent = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent listener");
    let pin = linked_pin(&pair.phone);
    assert!(
        pair.phone
            .record_coordinator_contact(
                &pin.vault_id,
                &pin.certificate_fingerprint,
                crate::vault::handoff::pairing::CoordinatorContact {
                    last_endpoint: Some(foreign.invitation.endpoint.clone()),
                    ..Default::default()
                },
            )
            .expect("record a foreign endpoint")
    );
    let pin = linked_pin(&pair.phone);
    let (_, identity) = load_tls_identity(&pair.phone).await.expect("identity");
    let started = std::time::Instant::now();
    let browsed = AtomicBool::new(false);
    let mut stream = connect_coordinator(
        &pair.phone,
        &pin,
        identity,
        &[silent.local_addr().expect("silent endpoint")],
        async {
            browsed.store(true, Ordering::Release);
            Vec::new()
        },
    )
    .await
    .expect("reach the pinned coordinator");
    assert!(
        started.elapsed() < COORDINATOR_CONNECT_TIMEOUT,
        "a silent endpoint must not hold the dial for its full timeout"
    );
    assert!(
        !browsed.load(Ordering::Acquire),
        "discovery waits until every candidate failed"
    );
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            known: Vec::new(),
        },
    ))
    .await
    .expect("send over the pinned stream");
    timeout_control(read_control(&mut stream))
        .await
        .expect("the coordinator answers over the pinned stream");
    let remembered = linked_pin(&pair.phone);
    assert_eq!(
        remembered.last_endpoint.as_deref(),
        Some(pair.invitation.endpoint.as_str())
    );
    assert_eq!(remembered.current_endpoint(), pair.invitation.endpoint);
}

#[tokio::test]
async fn a_coordinator_dial_that_reaches_no_endpoint_reports_every_attempt() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let closed_endpoint = || {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("closed endpoint")
    };
    let first = closed_endpoint();
    let second = closed_endpoint();
    let pin = CoordinatorPin {
        endpoint: first.to_string(),
        last_endpoint: None,
        reported_endpoints: Vec::new(),
        ..linked_pin(&pair.phone)
    };
    let (_, identity) = load_tls_identity(&pair.phone).await.expect("identity");
    let Err(error) = connect_coordinator(
        &pair.phone,
        &pin,
        identity,
        &[second],
        std::future::ready(vec![first]),
    )
    .await
    else {
        panic!("no endpoint is listening");
    };
    assert!(
        error.starts_with("could not connect to the coordinator at any known endpoint"),
        "{error}"
    );
    assert!(error.contains(&first.to_string()), "{error}");
    assert!(error.contains(&second.to_string()), "{error}");
    assert_eq!(
        error.matches(&first.to_string()).count(),
        1,
        "a discovered endpoint that was already tried is not dialed again: {error}"
    );
}

#[tokio::test]
async fn discovery_reaches_a_coordinator_that_moved_after_every_known_endpoint_failed() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind")
        .local_addr()
        .expect("closed endpoint");
    let pin = CoordinatorPin {
        endpoint: closed.to_string(),
        last_endpoint: None,
        reported_endpoints: Vec::new(),
        ..linked_pin(&pair.phone)
    };
    let moved = socket_address(&pair.invitation.endpoint);
    let (_, identity) = load_tls_identity(&pair.phone).await.expect("identity");
    let mut stream = connect_coordinator(
        &pair.phone,
        &pin,
        identity,
        &[],
        std::future::ready(vec![closed, moved]),
    )
    .await
    .expect("reach the coordinator at its discovered endpoint");
    timeout_control(write_control(
        &mut stream,
        &ControlMessage::SyncWait {
            protocol_version: PROTOCOL_VERSION,
            vault_id: "vault-1".to_string(),
            device_id: "device-phone".to_string(),
            known: Vec::new(),
        },
    ))
    .await
    .expect("send over the pinned stream");
    timeout_control(read_control(&mut stream))
        .await
        .expect("the coordinator answers over the pinned stream");
    assert_eq!(
        linked_pin(&pair.phone).last_endpoint.as_deref(),
        Some(pair.invitation.endpoint.as_str()),
        "the discovered endpoint that answered is dialed first next time"
    );
}

#[tokio::test]
async fn sync_states_refresh_the_remembered_coordinator_contact() {
    let pair = LocalPair::start().await;
    pair.enroll().await;
    let endpoints = vec![
        "192.168.1.20:43821".to_string(),
        "[fd00::20]:43821".to_string(),
    ];
    let coordinator = linked_pin(&pair.phone).device_id;
    let member = |device_id: &str, endpoint: &str| super::super::super::protocol::MemberEndpoint {
        device_id: device_id.to_string(),
        endpoint: endpoint.to_string(),
        observed_ms: 1_700_000_000_000,
    };
    let mut members: Vec<_> = endpoints
        .iter()
        .map(|endpoint| member(&coordinator, endpoint))
        .collect();
    members.push(member("device-other-member", "192.168.1.30:43821"));
    record_reported_contact(&pair.phone, &members);
    let pin = linked_pin(&pair.phone);
    assert_eq!(
        pin.reported_endpoints, endpoints,
        "only the coordinator's own endpoints refresh its pin"
    );

    record_reported_contact(&pair.phone, &[member(&coordinator, "8.8.8.8:43821")]);
    assert_eq!(
        linked_pin(&pair.phone).reported_endpoints,
        endpoints,
        "an unreportable endpoint list is not stored"
    );
}
