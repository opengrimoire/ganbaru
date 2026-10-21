//! Coordinator approval of devices that enroll with a pairing invitation.
//!
//! An invitation can leak, so the coordinator links nothing and sends nothing until its user
//! compares the code both devices show and approves the request. One request waits at a time, and
//! it keeps its unauthenticated connection slot for at most [`ENROLLMENT_APPROVAL_TIMEOUT`].

use super::pairing::random_token;
use super::protocol::DeviceKind;
use super::transport::ENROLLMENT_APPROVAL_TIMEOUT;
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::sync::oneshot;

pub(crate) const ENROLLMENT_DECLINED_MESSAGE: &str = "the coordinator declined this device";
pub(crate) const ENROLLMENT_UNANSWERED_MESSAGE: &str =
    "the coordinator did not approve this device in time";
pub(crate) const ENROLLMENT_BUSY_MESSAGE: &str =
    "another device is waiting for approval on the coordinator";

/// What the coordinator user sees while a device waits for approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PendingEnrollmentView {
    pub request_id: String,
    pub device_label: String,
    pub device_kind: DeviceKind,
    pub code: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnrollmentDecision {
    Approved,
    Declined,
}

struct PendingEnrollment {
    view: PendingEnrollmentView,
    decision: oneshot::Sender<EnrollmentDecision>,
}

/// The enrollment waiting for the coordinator user, shared by the listener and the commands.
#[derive(Clone)]
pub(crate) struct EnrollmentApprovals {
    pending: Arc<Mutex<Option<PendingEnrollment>>>,
    timeout: Duration,
}

impl Default for EnrollmentApprovals {
    fn default() -> Self {
        Self {
            pending: Arc::default(),
            timeout: ENROLLMENT_APPROVAL_TIMEOUT,
        }
    }
}

/// A started approval wait. The receiver closes without a decision when the request is cancelled.
pub(crate) struct ApprovalWait {
    pub request_id: String,
    pub decision: oneshot::Receiver<EnrollmentDecision>,
}

impl EnrollmentApprovals {
    #[cfg(test)]
    pub(crate) fn with_timeout(timeout: Duration) -> Self {
        Self {
            timeout,
            ..Self::default()
        }
    }

    /// How long a request waits for the user before the coordinator refuses it.
    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }

    fn lock(&self) -> Result<MutexGuard<'_, Option<PendingEnrollment>>, String> {
        self.pending
            .lock()
            .map_err(|_| "enrollment approval state is unavailable".to_string())
    }

    /// Starts waiting for the user, or refuses while another device is waiting.
    pub(crate) fn begin(
        &self,
        device_label: &str,
        device_kind: DeviceKind,
        code: String,
    ) -> Result<ApprovalWait, String> {
        let mut pending = self.lock()?;
        if pending.is_some() {
            return Err(ENROLLMENT_BUSY_MESSAGE.to_string());
        }
        let request_id = random_token("enrollment")?;
        let (sender, receiver) = oneshot::channel();
        *pending = Some(PendingEnrollment {
            view: PendingEnrollmentView {
                request_id: request_id.clone(),
                device_label: device_label.to_string(),
                device_kind,
                code,
            },
            decision: sender,
        });
        Ok(ApprovalWait {
            request_id,
            decision: receiver,
        })
    }

    /// The request the coordinator user should answer, if any.
    pub(crate) fn pending(&self) -> Result<Option<PendingEnrollmentView>, String> {
        Ok(self.lock()?.as_ref().map(|pending| pending.view.clone()))
    }

    /// Delivers the user's answer to the waiting request with this id.
    pub(crate) fn decide(
        &self,
        request_id: &str,
        decision: EnrollmentDecision,
    ) -> Result<(), String> {
        let mut pending = self.lock()?;
        if pending
            .as_ref()
            .is_none_or(|pending| pending.view.request_id != request_id)
        {
            return Err("this device is no longer waiting for approval".to_string());
        }
        let pending = pending.take().expect("checked pending enrollment");
        pending
            .decision
            .send(decision)
            .map_err(|_| "this device is no longer waiting for approval".to_string())
    }

    /// Clears a request that ended without a decision, such as a timeout or a closed connection.
    pub(crate) fn finish(&self, request_id: &str) {
        if let Ok(mut pending) = self.lock()
            && pending
                .as_ref()
                .is_some_and(|pending| pending.view.request_id == request_id)
        {
            pending.take();
        }
    }

    /// Cancels any waiting request, for example when a new invitation replaces the one it used.
    pub(crate) fn cancel(&self) {
        if let Ok(mut pending) = self.lock() {
            pending.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_request_waits_at_a_time_and_only_its_id_can_answer_it() {
        let approvals = EnrollmentApprovals::default();
        let mut wait = approvals
            .begin("Phone", DeviceKind::Phone, "AAAA-BBBB-CCCC".to_string())
            .expect("begin");
        assert_eq!(
            approvals
                .begin("Other", DeviceKind::Computer, "DDDD-EEEE-FFFF".to_string())
                .err()
                .as_deref(),
            Some(ENROLLMENT_BUSY_MESSAGE)
        );
        let view = approvals.pending().expect("pending").expect("waiting");
        assert_eq!(view.device_label, "Phone");
        assert_eq!(view.code, "AAAA-BBBB-CCCC");
        assert!(
            approvals
                .decide("enrollment-other", EnrollmentDecision::Approved)
                .is_err()
        );
        approvals
            .decide(&wait.request_id, EnrollmentDecision::Declined)
            .expect("decide");
        assert_eq!(wait.decision.try_recv(), Ok(EnrollmentDecision::Declined));
        assert!(approvals.pending().expect("pending").is_none());
        assert!(
            approvals
                .decide(&wait.request_id, EnrollmentDecision::Approved)
                .is_err()
        );
    }

    #[test]
    fn finishing_or_cancelling_frees_the_slot_without_a_decision() {
        let approvals = EnrollmentApprovals::default();
        let first = approvals
            .begin("Phone", DeviceKind::Phone, "AAAA-BBBB-CCCC".to_string())
            .expect("begin");
        approvals.finish("enrollment-other");
        assert!(approvals.pending().expect("pending").is_some());
        approvals.finish(&first.request_id);
        assert!(approvals.pending().expect("pending").is_none());

        let mut second = approvals
            .begin("Phone", DeviceKind::Phone, "AAAA-BBBB-CCCC".to_string())
            .expect("begin again");
        approvals.cancel();
        assert!(approvals.pending().expect("pending").is_none());
        assert!(second.decision.try_recv().is_err());
    }
}
