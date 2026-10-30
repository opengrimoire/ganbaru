//! The app's side of the coordinator and member sync listeners: the services each one gets and
//! the routes it hands requests outside sync to. The listeners themselves are
//! [`ganbaru_sync_replica::serve`].

use crate::vault::handoff::pairing::PairingManager;
use ganbaru_handoff::dial::LoopbackPolicy;
#[cfg(all(desktop, test))]
pub(super) use ganbaru_sync_replica::serve::accept_connections;
pub(super) use ganbaru_sync_replica::serve::limits;
#[cfg(desktop)]
use ganbaru_sync_replica::serve::send_error;
pub(crate) use ganbaru_sync_replica::serve::{
    CoordinatorListener, LanAdmission, MemberAdmissionSource,
};
use limits::ConnectionLimits;

#[cfg(desktop)]
mod dispatch;
#[cfg(desktop)]
mod member_bundles;
#[cfg(all(desktop, test))]
pub(super) use dispatch::serve_with_limits;
#[cfg(desktop)]
pub(crate) use dispatch::{
    CoordinatorServices, EnrollmentSealer, QuarantinedDevices, serve_with_coordinator,
};
#[cfg(desktop)]
pub(crate) use member_bundles::{
    MemberBundles, PreparedSnapshot, SnapshotPreparer, SnapshotRequest,
};

/// What a member listener serves and whom it admits.
#[derive(Clone)]
pub(crate) struct MemberServices {
    /// The hub that answers admitted sync requests.
    pub hub: crate::sync::hub::SyncHub,
    /// Admission from the selected log, read for every authenticated connection.
    pub admission: MemberAdmissionSource,
    /// Whether the network behind each local address may reach the listener at all.
    pub lan: LanAdmission,
    /// The snapshot copies a member desktop serves to enrolled devices. Phones serve none.
    #[cfg(desktop)]
    pub bundles: Option<MemberBundles>,
}

/// Serves `listener` as the sync listener of a member desktop until shutdown.
pub(crate) async fn serve_member_listener(
    listener: CoordinatorListener,
    manager: PairingManager,
    services: MemberServices,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    serve_member(
        listener,
        ConnectionLimits::default(),
        super::LOOPBACK_PEERS,
        manager,
        services,
        shutdown,
    )
    .await;
}

/// Serves `listener` as a member sync listener until shutdown.
pub(super) async fn serve_member(
    listener: CoordinatorListener,
    limits: ConnectionLimits,
    loopback: LoopbackPolicy,
    manager: PairingManager,
    services: MemberServices,
    shutdown: tokio::sync::oneshot::Receiver<()>,
) {
    let services = ganbaru_sync_replica::serve::MemberServices {
        hub: services.hub,
        admission: services.admission,
        lan: services.lan,
        #[cfg(desktop)]
        routes: services.bundles,
        #[cfg(not(desktop))]
        routes: ganbaru_sync_replica::serve::NoRoutes,
    };
    ganbaru_sync_replica::serve::serve_member(
        listener, limits, loopback, manager, services, shutdown,
    )
    .await;
}
