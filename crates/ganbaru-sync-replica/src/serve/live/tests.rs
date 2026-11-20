use super::*;
use ganbaru_handoff::protocol::{PROTOCOL_VERSION, read_control};
use ganbaru_sync::{AuthorizationView, EnrolledDevice};
use ganbaru_sync_contracts::{AdminPublicKey, DeviceKind, DevicePublicKey, DeviceRole};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};

const VAULT: &str = "vault-live";
const PHONE: &str = "device-phone";
const PHONE_CERTIFICATE: &[u8] = b"phone certificate";
const ASSIGNMENT: &str = "assignment-1";

fn enrolled(device_id: &str, certificate: &[u8], removed: bool, held: bool) -> EnrolledDevice {
    EnrolledDevice {
        device_id: device_id.to_string(),
        device_key: DevicePublicKey::from_bytes([3; 32]),
        tls_fingerprint: Sha256::digest(certificate).into(),
        label: None,
        kind: DeviceKind::Mobile,
        role: DeviceRole::Member,
        enrolled_revision: 2,
        removed_revision: removed.then_some(5),
        held,
        encryption_key: None,
    }
}

fn admission(phone: EnrolledDevice, quarantined: &[&str]) -> MemberAdmission {
    let view = AuthorizationView {
        epoch: 1,
        revision: 6,
        entry_hash: [7; 32],
        anchor_device_id: "device-desktop".to_string(),
        frozen_at: None,
        catch_up: None,
        charter: [8; 32],
        recovery_admin_key: AdminPublicKey::from_bytes([9; 32]),
        kit_admin_key: AdminPublicKey::from_bytes([9; 32]),
        retired_admin_keys: BTreeSet::new(),
        devices: vec![phone],
        discovery_key: [5; 32],
        previous_discovery_key: None,
        key_epoch: 0,
        recovery_encryption_key: None,
        pending_removal: None,
        server: None,
    };
    let quarantined = quarantined.iter().map(ToString::to_string).collect();
    MemberAdmission::new(VAULT, &view, &quarantined).served_by("device-desktop")
}

fn admitted_phone() -> MemberAdmission {
    admission(enrolled(PHONE, PHONE_CERTIFICATE, false, false), &[])
}

fn phone_fingerprint() -> String {
    certificate_fingerprint(PHONE_CERTIFICATE)
}

#[test]
fn admission_refuses_every_device_that_may_not_follow_the_vault() {
    let fingerprint = phone_fingerprint();
    let cases = [
        (
            admission(enrolled("device-other", b"other", false, false), &[]),
            MemberRefusal::Unknown,
        ),
        (
            admission(enrolled(PHONE, PHONE_CERTIFICATE, true, false), &[]),
            MemberRefusal::Removed,
        ),
        (
            admission(enrolled(PHONE, PHONE_CERTIFICATE, false, true), &[]),
            MemberRefusal::Held,
        ),
        (
            admission(enrolled(PHONE, PHONE_CERTIFICATE, false, false), &[PHONE]),
            MemberRefusal::Quarantined,
        ),
    ];
    for (admission, refusal) in cases {
        assert_eq!(
            admit(Ok(Some(admission)), &fingerprint, VAULT, PHONE),
            Err(refusal)
        );
    }
    assert_eq!(
        admit(
            Ok(Some(admitted_phone())),
            &fingerprint,
            "vault-other",
            PHONE
        ),
        Err(MemberRefusal::VaultMismatch)
    );
    assert_eq!(
        admit(
            Ok(Some(admitted_phone())),
            &fingerprint,
            VAULT,
            "device-desktop"
        ),
        Err(MemberRefusal::DeviceMismatch)
    );
    assert_eq!(
        admit(Ok(None), &fingerprint, VAULT, PHONE),
        Err(MemberRefusal::Unavailable)
    );
    assert_eq!(
        admit(Err("locked".to_string()), &fingerprint, VAULT, PHONE),
        Err(MemberRefusal::Unavailable)
    );
    assert_eq!(
        admit(Ok(Some(admitted_phone())), &fingerprint, VAULT, PHONE),
        Ok(())
    );
}

/// Admission the test changes while a read waits.
type SharedAdmission = Arc<Mutex<Result<Option<MemberAdmission>, String>>>;

/// Live routes over a scripted reader that answers each read with the next page.
struct Harness {
    routes: ChatLiveRoutes,
    admission: SharedAdmission,
    changes: watch::Sender<u64>,
    reads: Arc<AtomicUsize>,
    approvals: Arc<Mutex<Vec<ChatApprovalQuery>>>,
    answer: Arc<Mutex<Result<ChatApprovalAnswer, String>>>,
}

fn approval_request() -> ControlMessage {
    ControlMessage::ChatApprovalResolve {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT.to_string(),
        device_id: PHONE.to_string(),
        assignment_id: ASSIGNMENT.to_string(),
        request_id: "request-1".to_string(),
        decision: ChatRemoteApprovalDecision::Deny,
        provider_option_id: "deny".to_string(),
        command_id: "command-1".to_string(),
    }
}

impl Harness {
    fn new(pages: Vec<ChatLiveAnswer>) -> Self {
        let admission: SharedAdmission = Arc::new(Mutex::new(Ok(Some(admitted_phone()))));
        let source = admission.clone();
        let admission_source: MemberAdmissionSource = Arc::new(move || {
            let current = source.lock().expect("admission").clone();
            Box::pin(async move { current })
        });
        let reads = Arc::new(AtomicUsize::new(0));
        let counter = reads.clone();
        let pages = Arc::new(pages);
        let reader: ChatLiveReader = Arc::new(move |query: ChatLiveQuery| {
            assert_eq!(query.assignment_id, ASSIGNMENT);
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let answer = pages[index.min(pages.len() - 1)].clone();
            Box::pin(async move { Ok(answer) })
        });
        let (changes, receiver) = watch::channel(0);
        let approvals = Arc::new(Mutex::new(Vec::new()));
        let resolved = approvals.clone();
        let answer = Arc::new(Mutex::new(Ok(ChatApprovalAnswer::Resolved)));
        let scripted = answer.clone();
        let resolver: ChatApprovalResolver = Arc::new(move |query: ChatApprovalQuery| {
            resolved.lock().expect("approvals").push(query);
            let answer = scripted.lock().expect("answer").clone();
            Box::pin(async move { answer })
        });
        Self {
            routes: ChatLiveRoutes::new(admission_source, reader, resolver, receiver)
                .waiting_at_most(Duration::from_secs(2)),
            admission,
            changes,
            reads,
            approvals,
            answer,
        }
    }

    /// Resolves one approval and returns the frame the peer receives.
    async fn resolve(&self, certificate: Option<&[u8]>) -> (Result<(), String>, ControlMessage) {
        let (mut server, mut client) = tokio::io::duplex(64 * 1024);
        let served =
            serve_approval(&self.routes, &mut server, certificate, approval_request()).await;
        let response = read_control(&mut client).await.expect("response frame");
        (served, response)
    }

    /// Serves one read and returns the frame the peer receives.
    async fn read(
        &self,
        certificate: Option<&[u8]>,
        after_change: Option<u64>,
        wait: bool,
    ) -> (Result<(), String>, ControlMessage) {
        serve(&self.routes, certificate, after_change, wait).await
    }
}

async fn serve(
    routes: &ChatLiveRoutes,
    certificate: Option<&[u8]>,
    after_change: Option<u64>,
    wait: bool,
) -> (Result<(), String>, ControlMessage) {
    let (mut server, mut client) = tokio::io::duplex(2 * 1024 * 1024);
    let request = ControlMessage::ChatLiveRead {
        protocol_version: PROTOCOL_VERSION,
        vault_id: VAULT.to_string(),
        device_id: PHONE.to_string(),
        assignment_id: ASSIGNMENT.to_string(),
        after_change,
        wait,
    };
    let served = serve_read(routes, &mut server, certificate, request).await;
    let response = read_control(&mut client).await.expect("response frame");
    (served, response)
}

fn page(change_cursor: u64, settled: bool) -> ChatLiveAnswer {
    ChatLiveAnswer::Page(ChatLivePage {
        payload: format!("{{\"cursor\":{change_cursor}}}"),
        change_cursor,
        has_more: false,
        reset_required: false,
        settled,
    })
}

fn error_code(response: &ControlMessage) -> &str {
    match response {
        ControlMessage::Error { code, .. } => code,
        other => panic!("expected an error, got {other:?}"),
    }
}

fn cursor(response: &ControlMessage) -> u64 {
    match response {
        ControlMessage::ChatLive { change_cursor, .. } => *change_cursor,
        other => panic!("expected a live page, got {other:?}"),
    }
}

#[tokio::test]
async fn an_admitted_read_returns_the_page_without_waiting_when_it_moved() {
    let harness = Harness::new(vec![page(9, false)]);
    let (served, response) = harness.read(Some(PHONE_CERTIFICATE), Some(4), true).await;
    served.expect("served");
    assert_eq!(cursor(&response), 9);
    assert_eq!(harness.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_read_without_a_certificate_or_admission_is_refused_before_any_row_is_read() {
    let harness = Harness::new(vec![page(9, false)]);
    let (served, response) = harness.read(None, None, false).await;
    assert!(served.is_err());
    assert_eq!(error_code(&response), MemberRefusal::Unknown.code());

    *harness.admission.lock().expect("admission") = Ok(Some(admission(
        enrolled(PHONE, PHONE_CERTIFICATE, false, false),
        &[PHONE],
    )));
    let (served, response) = harness.read(Some(PHONE_CERTIFICATE), None, false).await;
    assert!(served.is_err());
    assert_eq!(error_code(&response), MemberRefusal::Quarantined.code());

    *harness.admission.lock().expect("admission") = Err("locked".to_string());
    let (_, response) = harness.read(Some(PHONE_CERTIFICATE), None, false).await;
    assert_eq!(error_code(&response), MemberRefusal::Unavailable.code());
    assert_eq!(harness.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn an_assignment_executed_elsewhere_is_answered_as_not_here() {
    let harness = Harness::new(vec![ChatLiveAnswer::NotHere]);
    let (served, response) = harness.read(Some(PHONE_CERTIFICATE), None, true).await;
    served.expect("served");
    assert_eq!(error_code(&response), CHAT_LIVE_NOT_HERE_CODE);
}

#[tokio::test]
async fn a_page_past_the_frame_bound_is_never_sent() {
    let harness = Harness::new(vec![ChatLiveAnswer::Page(ChatLivePage {
        payload: "x".repeat(ganbaru_handoff::protocol::MAX_CHAT_LIVE_PAYLOAD_BYTES + 1),
        change_cursor: 3,
        has_more: false,
        reset_required: false,
        settled: false,
    })]);
    let (_, response) = harness.read(Some(PHONE_CERTIFICATE), None, false).await;
    assert_eq!(error_code(&response), CHAT_LIVE_UNAVAILABLE_CODE);
}

#[tokio::test(start_paused = true)]
async fn a_waiting_read_is_answered_after_the_next_change() {
    let harness = Harness::new(vec![page(4, false), page(4, false), page(7, false)]);
    let bump = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        harness.changes.send_modify(|value| *value += 1);
        tokio::time::sleep(Duration::from_millis(100)).await;
        harness.changes.send_modify(|value| *value += 1);
    };
    let ((served, response), ()) =
        tokio::join!(harness.read(Some(PHONE_CERTIFICATE), Some(4), true), bump);
    served.expect("served");
    assert_eq!(cursor(&response), 7);
    assert_eq!(harness.reads.load(Ordering::SeqCst), 3);
}

#[tokio::test(start_paused = true)]
async fn a_waiting_read_is_answered_unchanged_at_the_wait_bound() {
    let harness = Harness::new(vec![page(4, false)]);
    let started = tokio::time::Instant::now();
    let (served, response) = harness.read(Some(PHONE_CERTIFICATE), Some(4), true).await;
    served.expect("served");
    assert_eq!(cursor(&response), 4);
    assert!(started.elapsed() >= Duration::from_secs(2));
    assert_eq!(harness.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_settled_execution_is_answered_without_waiting() {
    let harness = Harness::new(vec![page(4, true)]);
    let started = tokio::time::Instant::now();
    let (_, response) = harness.read(Some(PHONE_CERTIFICATE), Some(4), true).await;
    assert_eq!(cursor(&response), 4);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test(start_paused = true)]
async fn a_device_quarantined_while_its_read_waits_gets_no_further_rows() {
    let harness = Harness::new(vec![page(4, false), page(8, false)]);
    let quarantine = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        *harness.admission.lock().expect("admission") = Ok(Some(admission(
            enrolled(PHONE, PHONE_CERTIFICATE, false, false),
            &[PHONE],
        )));
        harness.changes.send_modify(|value| *value += 1);
    };
    let ((served, response), ()) = tokio::join!(
        harness.read(Some(PHONE_CERTIFICATE), Some(4), true),
        quarantine
    );
    assert!(served.is_err());
    assert_eq!(error_code(&response), MemberRefusal::Quarantined.code());
    assert_eq!(harness.reads.load(Ordering::SeqCst), 1);
}

#[tokio::test(start_paused = true)]
async fn a_device_removed_at_the_wait_bound_is_refused_the_stale_answer() {
    let harness = Harness::new(vec![page(4, false)]);
    let remove = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        *harness.admission.lock().expect("admission") = Ok(Some(admission(
            enrolled(PHONE, PHONE_CERTIFICATE, true, false),
            &[],
        )));
    };
    let ((_, response), ()) =
        tokio::join!(harness.read(Some(PHONE_CERTIFICATE), Some(4), true), remove);
    assert_eq!(error_code(&response), MemberRefusal::Removed.code());
}

#[tokio::test(start_paused = true)]
async fn a_new_wait_for_the_same_assignment_ends_the_one_it_replaces() {
    let harness = Harness::new(vec![page(4, false)]);
    let started = tokio::time::Instant::now();
    let first = async {
        let answer = harness.read(Some(PHONE_CERTIFICATE), Some(4), true).await;
        (answer, started.elapsed())
    };
    let second = async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        harness.read(Some(PHONE_CERTIFICATE), Some(4), true).await
    };
    let (((served, response), first_elapsed), (second_served, _)) = tokio::join!(first, second);
    served.expect("first served");
    second_served.expect("second served");
    assert_eq!(cursor(&response), 4);
    assert!(first_elapsed < Duration::from_secs(1));
    assert!(harness.routes.waits.lock().expect("waits").is_empty());
}

#[tokio::test]
async fn an_admitted_approval_reaches_the_resolver_with_what_the_phone_chose() {
    let harness = Harness::new(vec![page(1, false)]);
    let (served, response) = harness.resolve(Some(PHONE_CERTIFICATE)).await;
    served.expect("served");
    assert_eq!(response, ControlMessage::ChatApprovalResolved);
    let approvals = harness.approvals.lock().expect("approvals");
    assert_eq!(
        approvals.as_slice(),
        [ChatApprovalQuery {
            assignment_id: ASSIGNMENT.to_string(),
            request_id: "request-1".to_string(),
            decision: ChatRemoteApprovalDecision::Deny,
            provider_option_id: "deny".to_string(),
            command_id: "command-1".to_string(),
        }]
    );
}

#[tokio::test]
async fn an_approval_from_a_device_not_admitted_never_reaches_the_resolver() {
    let harness = Harness::new(vec![page(1, false)]);
    let (served, response) = harness.resolve(None).await;
    assert!(served.is_err());
    assert_eq!(error_code(&response), MemberRefusal::Unknown.code());

    *harness.admission.lock().expect("admission") = Ok(Some(admission(
        enrolled(PHONE, PHONE_CERTIFICATE, false, false),
        &[PHONE],
    )));
    let (served, response) = harness.resolve(Some(PHONE_CERTIFICATE)).await;
    assert!(served.is_err());
    assert_eq!(error_code(&response), MemberRefusal::Quarantined.code());

    *harness.admission.lock().expect("admission") = Ok(Some(admission(
        enrolled(PHONE, PHONE_CERTIFICATE, true, false),
        &[],
    )));
    let (_, response) = harness.resolve(Some(PHONE_CERTIFICATE)).await;
    assert_eq!(error_code(&response), MemberRefusal::Removed.code());
    assert!(harness.approvals.lock().expect("approvals").is_empty());
}

#[tokio::test]
async fn an_approval_answer_names_why_it_was_not_resolved() {
    let harness = Harness::new(vec![page(1, false)]);
    let cases = [
        (
            Ok(ChatApprovalAnswer::Stale),
            CHAT_APPROVAL_STALE_CODE,
            false,
        ),
        (
            Ok(ChatApprovalAnswer::NotHere),
            CHAT_LIVE_NOT_HERE_CODE,
            false,
        ),
        (Err("locked".to_string()), CHAT_LIVE_UNAVAILABLE_CODE, true),
    ];
    for (answer, code, expected_retryable) in cases {
        *harness.answer.lock().expect("answer") = answer;
        let (served, response) = harness.resolve(Some(PHONE_CERTIFICATE)).await;
        served.expect("served");
        match response {
            ControlMessage::Error {
                code: answered,
                retryable,
                ..
            } => {
                assert_eq!(answered, code);
                assert_eq!(retryable, expected_retryable);
            }
            other => panic!("expected an error, got {other:?}"),
        }
    }
}
