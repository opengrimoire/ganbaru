//! DNS-SD over mDNS on desktop: the coordinator advertises its hourly member tag while linked
//! devices of the active vault may look for it, and a linked device browses on demand when no
//! remembered coordinator endpoint answers.
//!
//! One `mdns-sd` responder serves the whole app run. It starts on first need, never answers on
//! loopback interfaces, and publishes only the reportable addresses it is given, never every
//! interface address. A dedicated thread forwards its address change events to the advertiser.

use super::local_addresses;
use super::pairing::{CoordinatorPin, PairingManager, certificate_fingerprint};
use super::protocol::decode_certificate_fingerprint;
use ganbaru_handoff::discovery::{
    self, Advertisement, DiscoveredRecord, DiscoveryKey, FINGERPRINT_BYTES, SERVICE_TYPE,
    TXT_VERSION_KEY,
};
use mdns_sd::{DaemonEvent, IfKind, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tauri::Runtime;
use tokio::sync::{oneshot, watch};

/// How long a linked device browses for its coordinator.
const BROWSE_DURATION: Duration = Duration::from_secs(3);
/// Shortest time between two browses for the same coordinator.
const DISCOVERY_INTERVAL: Duration = Duration::from_secs(60);
/// How long discovered endpoints stay dial candidates. They are never persisted.
const DISCOVERED_LIFETIME: Duration = Duration::from_secs(10 * 60);
/// Longest time the advertiser waits before it checks linked devices, the active vault, and
/// local addresses again.
const ADVERTISEMENT_RECHECK: Duration = Duration::from_secs(60);

/// The app's mDNS responder, started on first need.
static RESPONDER: Mutex<Option<Arc<Responder>>> = Mutex::new(None);
/// What the last browses of this device found.
static CLIENT_DISCOVERY: tokio::sync::Mutex<ClientDiscovery> =
    tokio::sync::Mutex::const_new(ClientDiscovery::new());

struct Responder {
    daemon: ServiceDaemon,
    /// Random host name of every advertisement in this app run.
    host_name: String,
    /// Counts the local address changes the responder reported.
    address_changes: watch::Sender<u64>,
}

impl Responder {
    fn start() -> Result<Self, String> {
        let daemon =
            ServiceDaemon::new().map_err(|error| format!("start the mDNS responder: {error}"))?;
        let started = Self::configure(&daemon);
        match started {
            Ok((host_name, address_changes)) => Ok(Self {
                daemon,
                host_name,
                address_changes,
            }),
            Err(error) => {
                if let Err(shutdown_error) = daemon.shutdown() {
                    eprintln!("stop the mDNS responder: {shutdown_error}");
                }
                Err(error)
            }
        }
    }

    fn configure(daemon: &ServiceDaemon) -> Result<(String, watch::Sender<u64>), String> {
        daemon
            .disable_interface(vec![IfKind::LoopbackV4, IfKind::LoopbackV6])
            .map_err(|error| format!("exclude loopback from the mDNS responder: {error}"))?;
        let host_name = discovery::random_host_name()?;
        let events = daemon
            .monitor()
            .map_err(|error| format!("monitor the mDNS responder: {error}"))?;
        let (address_changes, _) = watch::channel(0);
        let forwarded = address_changes.clone();
        std::thread::Builder::new()
            .name("ganbaru-mdns-events".to_string())
            .spawn(move || forward_responder_events(events, forwarded))
            .map_err(|error| format!("start the mDNS event thread: {error}"))?;
        Ok((host_name, address_changes))
    }

    fn register(&self, advertisement: &Advertisement) -> Result<(), String> {
        let info = ServiceInfo::new(
            SERVICE_TYPE,
            &advertisement.instance_name,
            &advertisement.host_name,
            advertisement.addresses.as_slice(),
            advertisement.port,
            advertisement.txt.as_slice(),
        )
        .map_err(|error| format!("describe the discovery advertisement: {error}"))?;
        self.daemon
            .register(info)
            .map_err(|error| format!("advertise the coordinator: {error}"))
    }

    fn unregister(&self, advertisement: &Advertisement) {
        let fullname = format!("{}.{SERVICE_TYPE}", advertisement.instance_name);
        if let Err(error) = self.daemon.unregister(&fullname) {
            eprintln!("withdraw the coordinator advertisement: {error}");
        }
    }

    /// Browses for at most `duration` and returns the resolved services whose instance name is
    /// in `expected`, stopping at the first one. Blocks the calling thread.
    fn browse(
        &self,
        duration: Duration,
        expected: &[String],
    ) -> Result<Vec<DiscoveredRecord>, String> {
        let events = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|error| format!("browse for the coordinator: {error}"))?;
        let deadline = Instant::now() + duration;
        let mut records = Vec::new();
        while let Ok(event) = events.recv_deadline(deadline) {
            if let ServiceEvent::ServiceResolved(service) = event
                && let Some(record) = discovered_record(&service, expected)
            {
                records.push(record);
                break;
            }
        }
        if let Err(error) = self.daemon.stop_browse(SERVICE_TYPE) {
            eprintln!("stop browsing for the coordinator: {error}");
        }
        Ok(records)
    }
}

/// Forwards address changes until the responder stops, and logs its errors.
fn forward_responder_events(
    events: mdns_sd::Receiver<DaemonEvent>,
    address_changes: watch::Sender<u64>,
) {
    while let Ok(event) = events.recv() {
        match event {
            DaemonEvent::IpAdd(_) | DaemonEvent::IpDel(_) => {
                address_changes.send_modify(|count| *count = count.wrapping_add(1));
            }
            DaemonEvent::Error(error) => eprintln!("mDNS responder: {error}"),
            _ => {}
        }
    }
}

/// The app's responder, started now if it is not running yet. A failed start is retried at the
/// next need.
fn responder() -> Result<Arc<Responder>, String> {
    let mut slot = RESPONDER
        .lock()
        .map_err(|_| "mDNS responder lock is unavailable".to_string())?;
    if let Some(responder) = slot.as_ref() {
        return Ok(responder.clone());
    }
    let responder = Arc::new(Responder::start()?);
    *slot = Some(responder.clone());
    Ok(responder)
}

/// The record of a resolved service whose instance name is in `expected`.
fn discovered_record(service: &ResolvedService, expected: &[String]) -> Option<DiscoveredRecord> {
    let instance_name = discovery::instance_name_of(service.get_fullname())?;
    if !expected.iter().any(|name| name == instance_name) {
        return None;
    }
    Some(DiscoveredRecord {
        instance_name: instance_name.to_string(),
        port: service.get_port(),
        addresses: service
            .get_addresses()
            .iter()
            .map(mdns_sd::ScopedIp::to_ip_addr)
            .collect(),
        version: service
            .get_property_val_str(TXT_VERSION_KEY)
            .map(str::to_string),
    })
}

fn since_epoch(now: SystemTime) -> Duration {
    now.duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
}

/// Advertises the coordinator listening on a port until it is dropped.
pub(crate) struct Advertiser {
    state: Arc<Mutex<AdvertiserState>>,
    stop: Option<oneshot::Sender<()>>,
}

#[derive(Default)]
struct AdvertiserState {
    stopped: bool,
    registered: Option<Registration>,
}

struct Registration {
    responder: Arc<Responder>,
    advertisement: Advertisement,
}

impl AdvertiserState {
    fn withdraw(&mut self) {
        if let Some(registration) = self.registered.take() {
            registration
                .responder
                .unregister(&registration.advertisement);
        }
    }
}

impl Advertiser {
    /// Starts advertising the coordinator listening on `port` whenever the active vault has a
    /// linked device. The advertisement follows the hourly tag, local address changes, linked
    /// devices, and the active vault.
    pub(crate) fn spawn<R: Runtime>(
        app: tauri::AppHandle<R>,
        manager: PairingManager,
        port: u16,
    ) -> Self {
        let state = Arc::new(Mutex::new(AdvertiserState::default()));
        let (stop, stopped) = oneshot::channel();
        tauri::async_runtime::spawn(advertise(app, manager, port, state.clone(), stopped));
        Self {
            state,
            stop: Some(stop),
        }
    }
}

impl Drop for Advertiser {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            state.withdraw();
        }
    }
}

async fn advertise<R: Runtime>(
    app: tauri::AppHandle<R>,
    manager: PairingManager,
    port: u16,
    state: Arc<Mutex<AdvertiserState>>,
    mut stopped: oneshot::Receiver<()>,
) {
    let mut address_changes: Option<watch::Receiver<u64>> = None;
    let mut last_error: Option<String> = None;
    loop {
        let now = since_epoch(SystemTime::now());
        match refresh_advertisement(&app, &manager, port, &state, now.as_secs()) {
            Ok(Some(responder)) if address_changes.is_none() => {
                address_changes = Some(responder.address_changes.subscribe());
            }
            Ok(_) => {}
            Err(error) => {
                if last_error.as_ref() != Some(&error) {
                    eprintln!("coordinator discovery advertisement: {error}");
                }
                last_error = Some(error);
            }
        }
        tokio::select! {
            _ = &mut stopped => return,
            () = tokio::time::sleep(next_check_delay(now)) => {}
            () = address_changed(&mut address_changes) => {}
        }
    }
}

/// Resolves when the responder reports a local address change, and never when there is no
/// responder to report one.
async fn address_changed(changes: &mut Option<watch::Receiver<u64>>) {
    let closed = match changes.as_mut() {
        Some(receiver) => receiver.changed().await.is_err(),
        None => true,
    };
    if closed {
        *changes = None;
        std::future::pending::<()>().await;
    }
}

/// Brings the registration in line with what should be advertised at `unix_seconds`, and
/// returns the responder of the current registration.
fn refresh_advertisement<R: Runtime>(
    app: &tauri::AppHandle<R>,
    manager: &PairingManager,
    port: u16,
    state: &Mutex<AdvertiserState>,
    unix_seconds: u64,
) -> Result<Option<Arc<Responder>>, String> {
    let identity = advertised_identity(app, manager)?;
    let desired = match identity {
        Some((key, fingerprint)) => {
            let addresses = local_addresses::reported_addresses();
            if addresses.is_empty() {
                None
            } else {
                let responder = responder()?;
                let advertisement = discovery::advertisement(
                    &key,
                    &fingerprint,
                    &responder.host_name,
                    port,
                    &addresses,
                    unix_seconds,
                );
                (!advertisement.addresses.is_empty()).then_some((responder, advertisement))
            }
        }
        None => None,
    };
    let mut state = state
        .lock()
        .map_err(|_| "discovery advertiser lock is unavailable".to_string())?;
    if state.stopped {
        return Ok(None);
    }
    let registered = state
        .registered
        .as_ref()
        .map(|registration| &registration.advertisement);
    match plan_registration(registered, desired.as_ref().map(|(_, ad)| ad)) {
        RegistrationChange::Keep => {}
        RegistrationChange::Withdraw => state.withdraw(),
        RegistrationChange::Register => {
            state.withdraw();
            if let Some((responder, advertisement)) = desired {
                responder.register(&advertisement)?;
                state.registered = Some(Registration {
                    responder,
                    advertisement,
                });
            }
        }
    }
    Ok(state
        .registered
        .as_ref()
        .map(|registration| registration.responder.clone()))
}

/// The discovery key and certificate fingerprint to advertise, or `None` while this device is
/// linked to another coordinator or the active vault has no linked device.
fn advertised_identity<R: Runtime>(
    app: &tauri::AppHandle<R>,
    manager: &PairingManager,
) -> Result<Option<(DiscoveryKey, [u8; FINGERPRINT_BYTES])>, String> {
    if manager.coordinator_pin()?.is_some() {
        return Ok(None);
    }
    let vault_id = crate::vault::active_vault_id(app)?;
    if manager.linked_peers_for(&vault_id)?.is_empty() {
        return Ok(None);
    }
    let key = DiscoveryKey::decode(&manager.discovery_key(&vault_id)?)?;
    let fingerprint =
        decode_certificate_fingerprint(&certificate_fingerprint(manager.certificate()?.as_ref()))
            .ok_or_else(|| "device certificate fingerprint is invalid".to_string())?;
    Ok(Some((key, fingerprint)))
}

/// What the advertiser does with its registration.
#[derive(Debug, PartialEq, Eq)]
enum RegistrationChange {
    Keep,
    /// Replace any current registration with the desired advertisement.
    Register,
    Withdraw,
}

fn plan_registration(
    registered: Option<&Advertisement>,
    desired: Option<&Advertisement>,
) -> RegistrationChange {
    match (registered, desired) {
        (None, None) => RegistrationChange::Keep,
        (Some(_), None) => RegistrationChange::Withdraw,
        (registered, Some(desired)) if registered == Some(desired) => RegistrationChange::Keep,
        (_, Some(_)) => RegistrationChange::Register,
    }
}

/// How long the advertiser sleeps after a check at `now` (time since the Unix epoch): until the
/// next tag starts, or [`ADVERTISEMENT_RECHECK`] when that comes first.
fn next_check_delay(now: Duration) -> Duration {
    let until_next_tag = Duration::from_secs(discovery::seconds_until_next_tag(now.as_secs()))
        .saturating_sub(Duration::from_nanos(u64::from(now.subsec_nanos())));
    until_next_tag.min(ADVERTISEMENT_RECHECK)
}

/// The coordinator a set of discovered endpoints belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
struct DiscoveryTarget {
    vault_id: String,
    certificate_fingerprint: String,
}

impl DiscoveryTarget {
    fn of(pin: &CoordinatorPin) -> Self {
        Self {
            vault_id: pin.vault_id.clone(),
            certificate_fingerprint: pin.certificate_fingerprint.clone(),
        }
    }
}

/// Endpoints the last browse found and when browsing may run again, kept in memory only.
#[derive(Debug)]
struct ClientDiscovery {
    target: Option<DiscoveryTarget>,
    last_browse: Option<Instant>,
    found: Vec<SocketAddr>,
    found_at: Option<Instant>,
}

impl ClientDiscovery {
    const fn new() -> Self {
        Self {
            target: None,
            last_browse: None,
            found: Vec::new(),
            found_at: None,
        }
    }

    /// The endpoints found for `target` that are still fresh at `now`.
    fn remembered(&self, target: &DiscoveryTarget, now: Instant) -> Vec<SocketAddr> {
        let fresh = self
            .found_at
            .is_some_and(|found_at| now.saturating_duration_since(found_at) < DISCOVERED_LIFETIME);
        if self.target.as_ref() == Some(target) && fresh {
            self.found.clone()
        } else {
            Vec::new()
        }
    }

    /// Whether a browse for `target` may start at `now`.
    fn may_browse(&self, target: &DiscoveryTarget, now: Instant) -> bool {
        self.target.as_ref() != Some(target)
            || self
                .last_browse
                .is_none_or(|last| now.saturating_duration_since(last) >= DISCOVERY_INTERVAL)
    }

    /// Notes a browse for `target` starting at `now`, forgetting another coordinator's results.
    fn begin_browse(&mut self, target: &DiscoveryTarget, now: Instant) {
        if self.target.as_ref() != Some(target) {
            self.target = Some(target.clone());
            self.found.clear();
            self.found_at = None;
        }
        self.last_browse = Some(now);
    }

    /// Keeps what a browse found. An empty result keeps earlier endpoints until they expire.
    fn record(&mut self, found: Vec<SocketAddr>, now: Instant) {
        if !found.is_empty() {
            self.found = found;
            self.found_at = Some(now);
        }
    }
}

/// Endpoints of `pin`'s coordinator that a recent browse found, to dial before browsing again.
pub(crate) async fn remembered_endpoints(pin: &CoordinatorPin) -> Vec<SocketAddr> {
    CLIENT_DISCOVERY
        .lock()
        .await
        .remembered(&DiscoveryTarget::of(pin), Instant::now())
}

/// Browses for `pin`'s coordinator when the pin carries a discovery key and no browse ran in
/// the last [`DISCOVERY_INTERVAL`], and returns the endpoints worth dialing. Within the
/// interval it returns what the last browse found instead.
pub(crate) async fn discover_coordinator(pin: &CoordinatorPin) -> Vec<SocketAddr> {
    let Some(key) = pin
        .discovery_key
        .as_deref()
        .and_then(|key| DiscoveryKey::decode(key).ok())
    else {
        return Vec::new();
    };
    let Some(fingerprint) = decode_certificate_fingerprint(&pin.certificate_fingerprint) else {
        return Vec::new();
    };
    let target = DiscoveryTarget::of(pin);
    let mut state = CLIENT_DISCOVERY.lock().await;
    let now = Instant::now();
    if !state.may_browse(&target, now) {
        return state.remembered(&target, now);
    }
    state.begin_browse(&target, now);
    let unix_seconds = since_epoch(SystemTime::now()).as_secs();
    let expected = discovery::expected_instance_names(&key, &[fingerprint], unix_seconds);
    let records = match browse(expected.clone()).await {
        Ok(records) => records,
        Err(error) => {
            eprintln!("coordinator discovery: {error}");
            Vec::new()
        }
    };
    state.record(
        discovery::match_candidates(&expected, &records),
        Instant::now(),
    );
    state.remembered(&target, Instant::now())
}

async fn browse(expected: Vec<String>) -> Result<Vec<DiscoveredRecord>, String> {
    let responder = responder()?;
    tauri::async_runtime::spawn_blocking(move || responder.browse(BROWSE_DURATION, &expected))
        .await
        .map_err(|error| format!("browse for the coordinator: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;

    const HOUR_START: Duration = Duration::from_secs(478_000 * 3600);

    fn advertisement_at(now: Duration, addresses: &[IpAddr]) -> Advertisement {
        discovery::advertisement(
            &DiscoveryKey::from_bytes([7; 32]),
            &[0xab; FINGERPRINT_BYTES],
            "0011223344556677.local.",
            43_821,
            addresses,
            now.as_secs(),
        )
    }

    fn lan() -> Vec<IpAddr> {
        vec!["192.168.1.20".parse().expect("address")]
    }

    #[test]
    fn the_discovery_advertiser_registers_a_new_tag_at_each_hour_boundary_only() {
        let start = HOUR_START + Duration::from_millis(3_599_250);
        let end = start + Duration::from_secs(2 * 3600);
        let mut now = start;
        let mut registered = advertisement_at(now, &lan());
        let mut registrations = Vec::new();
        let mut checks = 0;
        loop {
            let next = now + next_check_delay(now);
            if next > end {
                break;
            }
            now = next;
            checks += 1;
            let desired = advertisement_at(now, &lan());
            if plan_registration(Some(&registered), Some(&desired)) == RegistrationChange::Register
            {
                registrations.push(now);
                registered = desired;
            }
        }
        assert_eq!(
            registrations,
            vec![
                HOUR_START + Duration::from_secs(3600),
                HOUR_START + Duration::from_secs(2 * 3600),
            ]
        );
        assert!(checks <= 2 * 60 + 3, "{checks} checks in two hours");
        assert_eq!(
            next_check_delay(start),
            Duration::from_millis(750),
            "the tag switch is not delayed by the recheck interval"
        );
        assert_eq!(next_check_delay(HOUR_START), ADVERTISEMENT_RECHECK);
    }

    #[test]
    fn the_discovery_advertiser_follows_addresses_and_withdraws_without_linked_devices() {
        let now = HOUR_START;
        let registered = advertisement_at(now, &lan());
        let moved = advertisement_at(now, &["10.0.0.5".parse().expect("address")]);
        assert_eq!(
            plan_registration(Some(&registered), Some(&moved)),
            RegistrationChange::Register
        );
        assert_eq!(
            plan_registration(None, Some(&registered)),
            RegistrationChange::Register
        );
        assert_eq!(
            plan_registration(Some(&registered), None),
            RegistrationChange::Withdraw
        );
        assert_eq!(plan_registration(None, None), RegistrationChange::Keep);
    }

    fn target(vault_id: &str) -> DiscoveryTarget {
        DiscoveryTarget {
            vault_id: vault_id.to_string(),
            certificate_fingerprint: "a".repeat(64),
        }
    }

    #[test]
    fn client_discovery_browses_at_most_once_a_minute_per_coordinator() {
        let mut discovery = ClientDiscovery::new();
        let first = target("vault-1");
        let start = Instant::now();
        assert!(discovery.may_browse(&first, start));
        discovery.begin_browse(&first, start);
        assert!(!discovery.may_browse(&first, start + Duration::from_secs(59)));
        assert!(discovery.may_browse(&first, start + DISCOVERY_INTERVAL));
        assert!(
            discovery.may_browse(&target("vault-2"), start + Duration::from_secs(1)),
            "another coordinator is not held back by this one's browse"
        );
    }

    #[test]
    fn client_discovery_keeps_found_endpoints_for_ten_minutes_and_per_coordinator() {
        let mut discovery = ClientDiscovery::new();
        let first = target("vault-1");
        let found: Vec<SocketAddr> = vec!["192.168.1.20:43821".parse().expect("endpoint")];
        let start = Instant::now();
        discovery.begin_browse(&first, start);
        discovery.record(found.clone(), start);
        assert_eq!(
            discovery.remembered(&first, start + Duration::from_secs(599)),
            found
        );
        assert!(
            discovery
                .remembered(&first, start + DISCOVERED_LIFETIME)
                .is_empty()
        );
        assert!(discovery.remembered(&target("vault-2"), start).is_empty());

        let later = start + DISCOVERY_INTERVAL;
        discovery.begin_browse(&first, later);
        discovery.record(Vec::new(), later);
        assert_eq!(
            discovery.remembered(&first, later),
            found,
            "an empty browse keeps earlier endpoints until they expire"
        );

        discovery.begin_browse(&target("vault-2"), later);
        assert!(
            discovery.remembered(&first, later).is_empty(),
            "browsing for another coordinator forgets this one"
        );
    }
}
