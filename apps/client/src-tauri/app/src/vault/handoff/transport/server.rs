//! Listening sockets, connection admission, and the listeners built on them: the desktop
//! coordinator listener and the member sync listener that desktops and foreground phones run.

use super::timeout_control;
use crate::vault::handoff::pairing::PairingManager;
use crate::vault::handoff::protocol::{ControlMessage, write_control};
pub(crate) use admission::LanAdmission;
use ganbaru_handoff::dial::{LoopbackPolicy, admit_inbound_peer};
use limits::ConnectionLimits;
pub(crate) use listener::CoordinatorListener;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;
use tokio::net::TcpStream;

pub(super) mod admission;
mod client_verifier;
#[cfg(desktop)]
mod dispatch;
pub(super) mod limits;
mod listener;
mod member;
#[cfg(all(desktop, test))]
pub(super) use dispatch::serve_with_limits;
#[cfg(desktop)]
pub(crate) use dispatch::{
    CoordinatorServices, EnrollmentSealer, QuarantinedDevices, serve_with_coordinator,
};
pub(super) use member::serve_member;
pub(crate) use member::{MemberAdmissionSource, MemberServices};

const ACCEPT_BACKOFF_MIN: Duration = Duration::from_millis(10);
const ACCEPT_BACKOFF_MAX: Duration = Duration::from_secs(1);
/// Error code for a linked device turned away because the authenticated budget is full.
pub(super) const COORDINATOR_BUSY_CODE: &str = "coordinator_busy";

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

/// The accepted socket's own address when the shared address policy admits a peer at `source`
/// on it. A socket whose local address cannot be read is refused, so this gate never fails
/// open.
fn admitted_local_address(
    socket: &TcpStream,
    source: IpAddr,
    loopback: LoopbackPolicy,
) -> Option<SocketAddr> {
    socket
        .local_addr()
        .ok()
        .filter(|local| admit_inbound_peer(source, local.ip(), loopback).is_ok())
}

/// Accepts until shutdown. Accept errors such as descriptor or buffer exhaustion and aborted
/// handshakes are transient, so the loop logs them and backs off instead of stopping linking.
pub(super) async fn accept_connections<S, A, F>(
    mut accept: A,
    mut shutdown: tokio::sync::oneshot::Receiver<()>,
    mut on_accepted: impl FnMut(S, SocketAddr),
) where
    A: FnMut() -> F,
    F: Future<Output = std::io::Result<(S, SocketAddr)>>,
{
    let mut backoff = ACCEPT_BACKOFF_MIN;
    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = accept() => match accepted {
                Ok((socket, source)) => {
                    backoff = ACCEPT_BACKOFF_MIN;
                    on_accepted(socket, source);
                }
                Err(error) => {
                    eprintln!("vault handoff coordinator accept failed, retrying: {error}");
                    tokio::select! {
                        _ = &mut shutdown => break,
                        () = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(ACCEPT_BACKOFF_MAX);
                }
            },
        }
    }
}

async fn send_error<W>(
    writer: &mut W,
    code: &str,
    message: &str,
    retryable: bool,
) -> Result<(), String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    timeout_control(write_control(
        writer,
        &ControlMessage::Error {
            code: code.to_string(),
            message: message.to_string(),
            retryable,
        },
    ))
    .await
}
