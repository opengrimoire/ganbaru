//! Dual-stack wildcard sockets for the coordinator listener.
//!
//! The coordinator listens on every local address so a new lease, another Wi-Fi, or an overlay
//! interface needs no rebind. IPv4 and IPv6 use separate sockets, with `IPV6_V6ONLY` set on the
//! IPv6 one, so every peer arrives with its real address family and the address policy sees
//! no IPv4-mapped forms. Which networks may connect is decided per connection by the address
//! policy and admission, keyed by the address each connection arrived on.

use socket2::{Domain, Protocol, Socket, Type};
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use tokio::net::{TcpListener, TcpStream};

/// Pending connection backlog for each socket.
const LISTEN_BACKLOG: i32 = 1024;

/// The coordinator's listening sockets: IPv4 always, IPv6 when the host supports it.
pub(crate) struct CoordinatorListener {
    ipv4: TcpListener,
    ipv6: Option<TcpListener>,
}

impl From<TcpListener> for CoordinatorListener {
    /// One socket on any address, for listeners that serve a single family.
    fn from(listener: TcpListener) -> Self {
        Self {
            ipv4: listener,
            ipv6: None,
        }
    }
}

impl CoordinatorListener {
    /// Binds the IPv4 and IPv6 wildcard addresses on `port`. The listener runs on IPv4 alone
    /// when the IPv6 socket cannot be bound, for example when IPv6 is disabled; it fails only
    /// when IPv4 fails.
    pub(crate) fn bind(port: u16) -> io::Result<Self> {
        Self::bind_with(port, bind_wildcard)
    }

    /// Binds with `bind`, which turns a wildcard address into a listening socket. IPv4 binds
    /// first and IPv6 takes the port it got, so both sockets share one port even when `port`
    /// is 0.
    fn bind_with(
        port: u16,
        bind: impl Fn(SocketAddr) -> io::Result<TcpListener>,
    ) -> io::Result<Self> {
        let ipv4 = bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))?;
        let port = ipv4.local_addr()?.port();
        let ipv6 = match bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port))) {
            Ok(listener) => Some(listener),
            Err(error) => {
                eprintln!("vault handoff coordinator listens on IPv4 only: {error}");
                None
            }
        };
        Ok(Self { ipv4, ipv6 })
    }

    /// The port both sockets listen on.
    pub(crate) fn port(&self) -> io::Result<u16> {
        Ok(self.ipv4.local_addr()?.port())
    }

    /// Whether an IPv6 socket is listening.
    #[cfg(test)]
    pub(crate) fn has_ipv6(&self) -> bool {
        self.ipv6.is_some()
    }

    /// Accepts the next connection from either socket. Both accept futures are cancel safe, so
    /// the one that loses the race drops nothing.
    pub(crate) async fn accept(&self) -> io::Result<(TcpStream, SocketAddr)> {
        let Some(ipv6) = &self.ipv6 else {
            return self.ipv4.accept().await;
        };
        tokio::select! {
            accepted = self.ipv4.accept() => accepted,
            accepted = ipv6.accept() => accepted,
        }
    }
}

/// One nonblocking listening socket on `address`. IPv6 sockets refuse IPv4-mapped traffic.
/// `SO_REUSEADDR` is set outside Windows, as Tokio does, so a restarted coordinator can bind
/// while old connections linger; on Windows it would let another process take the port.
fn bind_wildcard(address: SocketAddr) -> io::Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(address),
        Type::STREAM,
        Some(Protocol::TCP),
    )?;
    if address.is_ipv6() {
        socket.set_only_v6(true)?;
    }
    #[cfg(not(windows))]
    socket.set_reuse_address(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&address.into())?;
    socket.listen(LISTEN_BACKLOG)?;
    TcpListener::from_std(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    async fn accepted_local_address(listener: &CoordinatorListener, target: SocketAddr) -> IpAddr {
        let (_client, accepted) = tokio::join!(TcpStream::connect(target), listener.accept());
        let (socket, _) = accepted.expect("accept");
        socket.local_addr().expect("local address").ip()
    }

    fn ipv6_loopback_available() -> bool {
        std::net::TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).is_ok()
    }

    #[tokio::test]
    async fn both_families_reach_one_listener_on_one_port() {
        let listener = CoordinatorListener::bind(0).expect("bind");
        let port = listener.port().expect("port");
        assert_eq!(
            accepted_local_address(&listener, SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await,
            IpAddr::from(Ipv4Addr::LOCALHOST)
        );
        if !ipv6_loopback_available() {
            assert!(
                !listener.has_ipv6(),
                "IPv6 binds only where the host has it"
            );
            return;
        }
        assert!(listener.has_ipv6());
        assert_eq!(
            accepted_local_address(&listener, SocketAddr::from((Ipv6Addr::LOCALHOST, port))).await,
            IpAddr::from(Ipv6Addr::LOCALHOST),
            "IPv6 peers keep their own family on the IPv6 socket"
        );
    }

    #[tokio::test]
    async fn a_failed_ipv6_socket_leaves_an_ipv4_listener() {
        let listener = CoordinatorListener::bind_with(0, |address| {
            if address.is_ipv6() {
                Err(io::Error::from(io::ErrorKind::AddrNotAvailable))
            } else {
                bind_wildcard(address)
            }
        })
        .expect("IPv4 alone still listens");
        assert!(!listener.has_ipv6());
        let port = listener.port().expect("port");
        assert_eq!(
            accepted_local_address(&listener, SocketAddr::from((Ipv4Addr::LOCALHOST, port))).await,
            IpAddr::from(Ipv4Addr::LOCALHOST)
        );
    }

    #[tokio::test]
    async fn a_failed_ipv4_socket_fails_the_listener() {
        let attempted_ipv6 = std::sync::atomic::AtomicBool::new(false);
        let result = CoordinatorListener::bind_with(0, |address| {
            if address.is_ipv6() {
                attempted_ipv6.store(true, std::sync::atomic::Ordering::SeqCst);
                return bind_wildcard(address);
            }
            Err(io::Error::from(io::ErrorKind::AddrInUse))
        });
        assert!(result.is_err());
        assert!(!attempted_ipv6.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn the_ipv6_socket_refuses_ipv4_mapped_traffic() {
        if !ipv6_loopback_available() {
            return;
        }
        let socket = bind_wildcard(SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))).expect("bind");
        assert!(
            socket2::SockRef::from(&socket)
                .only_v6()
                .expect("read IPV6_V6ONLY")
        );
    }
}
