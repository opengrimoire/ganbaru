//! DNS-SD discovery records for finding a coordinator whose address changed.
//!
//! A coordinator advertises `_ganbaru._tcp` over mDNS under an instance name that is an hourly
//! member tag: an HMAC of its certificate fingerprint and the current hour under the vault's
//! discovery key. Linked devices know the key and the fingerprint, so they compute the names to
//! look for; any other device sees only that a coordinator exists, a name that changes every
//! hour, a random host name, and the advertised LAN addresses. This module holds the record
//! format and the checks shared by every browser and advertiser, independent of the mDNS
//! implementation that carries them.

use crate::dial::{reportable_address, validate_dial_endpoint};
use crate::protocol::DISCOVERY_KEY_BYTES;
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use std::fmt;
use std::net::{IpAddr, SocketAddr};

/// UDP port of multicast DNS.
pub const MDNS_PORT: u16 = 5353;
/// DNS-SD service type and domain that coordinators advertise.
pub const SERVICE_TYPE: &str = "_ganbaru._tcp.local.";
/// TXT key that carries the record format version.
pub const TXT_VERSION_KEY: &str = "v";
/// Record format version this build advertises and accepts.
pub const TXT_VERSION: &str = "1";
/// Most endpoints one discovery returns.
pub const MAX_DISCOVERED_ENDPOINTS: usize = 8;
/// Most addresses one advertisement carries.
pub const MAX_ADVERTISED_ADDRESSES: usize = 8;

/// Domain separation prefix of the member tag HMAC input.
const MEMBER_TAG_DOMAIN: &[u8] = b"ganbaru-sync/discovery/v1";
/// Bytes of the HMAC output kept in a member tag.
const MEMBER_TAG_BYTES: usize = 16;
/// Length of one member tag bucket.
const TAG_HOUR_SECONDS: u64 = 3600;
/// Random bytes in an advertised host name.
const HOST_NAME_BYTES: usize = 8;
/// Bytes of a certificate fingerprint.
pub const FINGERPRINT_BYTES: usize = 32;

/// The secret a vault's linked devices share for recognizing their coordinator's
/// advertisements. It is never sent over mDNS.
#[derive(Clone, PartialEq, Eq)]
pub struct DiscoveryKey([u8; DISCOVERY_KEY_BYTES]);

impl DiscoveryKey {
    /// Wraps raw key bytes.
    pub fn from_bytes(bytes: [u8; DISCOVERY_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// A fresh key from the secure random source.
    pub fn generate() -> Result<Self, String> {
        let mut bytes = [0_u8; DISCOVERY_KEY_BYTES];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| "generate a discovery key".to_string())?;
        Ok(Self(bytes))
    }

    /// Raw key bytes.
    pub fn as_bytes(&self) -> &[u8; DISCOVERY_KEY_BYTES] {
        &self.0
    }

    /// Decodes the unpadded base64url form that sync states and pairing state carry.
    pub fn decode(encoded: &str) -> Result<Self, String> {
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, encoded)
                .map_err(|_| "discovery key is invalid".to_string())?;
        let bytes: [u8; DISCOVERY_KEY_BYTES] = decoded
            .try_into()
            .map_err(|_| "discovery key is invalid".to_string())?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for DiscoveryKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("DiscoveryKey(..)")
    }
}

/// The member tag of the device with certificate `fingerprint` for the hour bucket `hour`
/// (`unix_seconds / 3600`): 16 bytes of HMAC-SHA256 under `key`, as 32 lowercase hex digits.
pub fn member_tag(key: &DiscoveryKey, fingerprint: &[u8; FINGERPRINT_BYTES], hour: u64) -> String {
    let key = hmac::Key::new(hmac::HMAC_SHA256, &key.0);
    let mut context = hmac::Context::with_key(&key);
    context.update(MEMBER_TAG_DOMAIN);
    context.update(&[0]);
    context.update(fingerprint);
    context.update(&hour.to_be_bytes());
    lower_hex(&context.sign().as_ref()[..MEMBER_TAG_BYTES])
}

/// The hour bucket that holds `unix_seconds`.
pub fn tag_hour(unix_seconds: u64) -> u64 {
    unix_seconds / TAG_HOUR_SECONDS
}

/// Seconds from `unix_seconds` until the next hour bucket starts, when every advertiser
/// switches to its next tag. Always at least one.
pub fn seconds_until_next_tag(unix_seconds: u64) -> u64 {
    TAG_HOUR_SECONDS - unix_seconds % TAG_HOUR_SECONDS
}

/// The instance names that the devices with `fingerprints` may advertise at `unix_seconds`:
/// their tags for the previous, current, and next hour, so an hour of clock skew between
/// advertiser and browser still matches.
pub fn expected_instance_names(
    key: &DiscoveryKey,
    fingerprints: &[[u8; FINGERPRINT_BYTES]],
    unix_seconds: u64,
) -> Vec<String> {
    let hour = tag_hour(unix_seconds);
    let hours = [hour.checked_sub(1), Some(hour), hour.checked_add(1)];
    let mut names = Vec::new();
    for fingerprint in fingerprints {
        for hour in hours.into_iter().flatten() {
            let name = member_tag(key, fingerprint, hour);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// One DNS-SD service registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Advertisement {
    /// The member tag of the current hour.
    pub instance_name: String,
    /// Random host name of the form `<16 hex>.local.`, so the operating system host name never
    /// appears on the network.
    pub host_name: String,
    pub port: u16,
    /// Reportable addresses only, at most [`MAX_ADVERTISED_ADDRESSES`].
    pub addresses: Vec<IpAddr>,
    /// TXT properties as key and value pairs.
    pub txt: Vec<(String, String)>,
}

/// The advertisement of the device with certificate `fingerprint` listening on `port` at
/// `unix_seconds`. Addresses that may not be reported to other devices (loopback, IPv6
/// link-local, public) are left out, as are duplicates past the first
/// [`MAX_ADVERTISED_ADDRESSES`].
pub fn advertisement(
    key: &DiscoveryKey,
    fingerprint: &[u8; FINGERPRINT_BYTES],
    host_name: &str,
    port: u16,
    addresses: &[IpAddr],
    unix_seconds: u64,
) -> Advertisement {
    let mut reportable = Vec::new();
    for address in addresses.iter().map(IpAddr::to_canonical) {
        if reportable.len() == MAX_ADVERTISED_ADDRESSES {
            break;
        }
        if reportable_address(address).is_ok() && !reportable.contains(&address) {
            reportable.push(address);
        }
    }
    Advertisement {
        instance_name: member_tag(key, fingerprint, tag_hour(unix_seconds)),
        host_name: host_name.to_string(),
        port,
        addresses: reportable,
        txt: vec![(TXT_VERSION_KEY.to_string(), TXT_VERSION.to_string())],
    }
}

/// A random host name `<16 hex>.local.`, chosen once per responder start.
pub fn random_host_name() -> Result<String, String> {
    let mut bytes = [0_u8; HOST_NAME_BYTES];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "generate a discovery host name".to_string())?;
    Ok(format!("{}.local.", lower_hex(&bytes)))
}

/// One resolved DNS-SD service, as any browser reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredRecord {
    /// The instance label without the service type and domain.
    pub instance_name: String,
    pub port: u16,
    pub addresses: Vec<IpAddr>,
    /// The TXT `v` value, when present.
    pub version: Option<String>,
}

/// The instance label of a DNS-SD full name `<instance>._ganbaru._tcp.local.`, or `None` for
/// another service type.
pub fn instance_name_of(fullname: &str) -> Option<&str> {
    let instance = fullname.strip_suffix(SERVICE_TYPE)?.strip_suffix('.')?;
    (!instance.is_empty()).then_some(instance)
}

/// The endpoints worth dialing from `records`: only records whose instance name is in
/// `expected` and whose version this build reads, and only addresses a coordinator may report
/// about itself, deduplicated, IPv4 first, at most [`MAX_DISCOVERED_ENDPOINTS`]. The dialer still
/// authorizes each endpoint against its own links before connecting.
pub fn match_candidates(expected: &[String], records: &[DiscoveredRecord]) -> Vec<SocketAddr> {
    let mut endpoints = Vec::new();
    for record in records {
        if !expected.contains(&record.instance_name)
            || record.version.as_deref() != Some(TXT_VERSION)
        {
            continue;
        }
        let mut addresses: Vec<IpAddr> =
            record.addresses.iter().map(IpAddr::to_canonical).collect();
        addresses.sort_by_key(|address| (address.is_ipv6(), *address));
        for address in addresses {
            let endpoint = SocketAddr::new(address, record.port);
            if endpoints.len() == MAX_DISCOVERED_ENDPOINTS {
                return endpoints;
            }
            if validate_dial_endpoint(endpoint).is_ok()
                && reportable_address(address).is_ok()
                && !endpoints.contains(&endpoint)
            {
                endpoints.push(endpoint);
            }
        }
    }
    endpoints
}

fn lower_hex(bytes: &[u8]) -> String {
    use fmt::Write;
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    /// Hour bucket 478000 starts at this Unix time.
    const HOUR_START: u64 = 478_000 * 3600;

    fn sequential_key() -> DiscoveryKey {
        DiscoveryKey::from_bytes(std::array::from_fn(|index| index as u8))
    }

    fn fingerprint() -> [u8; FINGERPRINT_BYTES] {
        [0xab; FINGERPRINT_BYTES]
    }

    fn record(instance_name: &str, port: u16, addresses: &[&str]) -> DiscoveredRecord {
        DiscoveredRecord {
            instance_name: instance_name.to_string(),
            port,
            addresses: addresses
                .iter()
                .map(|address| address.parse().expect("address"))
                .collect(),
            version: Some(TXT_VERSION.to_string()),
        }
    }

    fn endpoints(values: &[&str]) -> Vec<SocketAddr> {
        values
            .iter()
            .map(|value| value.parse().expect("endpoint"))
            .collect()
    }

    #[test]
    fn member_tags_match_the_golden_vectors() {
        let key = sequential_key();
        assert_eq!(
            member_tag(&key, &fingerprint(), 0),
            "f31e3dff57527ac7003a76b95528d514"
        );
        assert_eq!(
            member_tag(&key, &fingerprint(), 478_000),
            "1fbac9365d94a9541d55a6b31eb89632"
        );
        assert_eq!(
            member_tag(&key, &fingerprint(), 478_001),
            "e84d37a36eeffce40acb866bb739f3f1"
        );
        let other_key = DiscoveryKey::from_bytes([0x42; DISCOVERY_KEY_BYTES]);
        let other_fingerprint = std::array::from_fn(|index| 32 + index as u8);
        assert_eq!(
            member_tag(&other_key, &other_fingerprint, 478_000),
            "4a8a8b500426058c1f3988ca2be261d0"
        );
    }

    #[test]
    fn the_base64url_key_decodes_to_the_same_tag_and_malformed_keys_are_refused() {
        let key = DiscoveryKey::decode("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8")
            .expect("decode key");
        assert_eq!(key, sequential_key());
        assert!(DiscoveryKey::decode("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHg").is_err());
        assert!(DiscoveryKey::decode("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=").is_err());
        assert!(DiscoveryKey::decode("not a key").is_err());
        assert_eq!(format!("{key:?}"), "DiscoveryKey(..)");
    }

    #[test]
    fn browsers_accept_one_hour_of_skew_either_way_and_refuse_two() {
        let key = sequential_key();
        let advertised_at = |hour: u64| {
            advertisement(
                &key,
                &fingerprint(),
                "h.local.",
                43_821,
                &[],
                hour * 3600 + 17,
            )
            .instance_name
        };
        let expected = expected_instance_names(&key, &[fingerprint()], HOUR_START + 1800);
        assert_eq!(expected.len(), 3);
        for hour in [477_999, 478_000, 478_001] {
            assert!(expected.contains(&advertised_at(hour)), "hour {hour}");
        }
        for hour in [477_998, 478_002] {
            assert!(!expected.contains(&advertised_at(hour)), "hour {hour}");
        }
    }

    #[test]
    fn expected_names_cover_every_fingerprint_and_start_at_hour_zero() {
        let key = sequential_key();
        let second = [0xcd; FINGERPRINT_BYTES];
        let names = expected_instance_names(&key, &[fingerprint(), second], HOUR_START);
        assert_eq!(names.len(), 6);
        assert!(names.contains(&member_tag(&key, &second, 478_001)));
        let at_epoch = expected_instance_names(&key, &[fingerprint()], 0);
        assert_eq!(
            at_epoch,
            vec![
                member_tag(&key, &fingerprint(), 0),
                member_tag(&key, &fingerprint(), 1)
            ]
        );
    }

    #[test]
    fn tags_change_at_each_hour_boundary() {
        assert_eq!(seconds_until_next_tag(HOUR_START), 3600);
        assert_eq!(seconds_until_next_tag(HOUR_START + 3599), 1);
        assert_eq!(tag_hour(HOUR_START + 3599), 478_000);
        assert_eq!(tag_hour(HOUR_START + 3600), 478_001);
    }

    #[test]
    fn advertisements_carry_the_tag_version_and_reportable_addresses_only() {
        let key = sequential_key();
        let addresses: Vec<IpAddr> = [
            "192.168.1.20",
            "127.0.0.1",
            "fe80::1",
            "8.8.8.8",
            "fd00::20",
            "::ffff:192.168.1.20",
            "100.64.0.7",
        ]
        .iter()
        .map(|address| address.parse().expect("address"))
        .collect();
        let advertisement = advertisement(
            &key,
            &fingerprint(),
            "0011223344556677.local.",
            43_821,
            &addresses,
            HOUR_START + 5,
        );
        assert_eq!(
            advertisement,
            Advertisement {
                instance_name: "1fbac9365d94a9541d55a6b31eb89632".to_string(),
                host_name: "0011223344556677.local.".to_string(),
                port: 43_821,
                addresses: vec![
                    IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
                    "fd00::20".parse().expect("address"),
                    IpAddr::V4(Ipv4Addr::new(100, 64, 0, 7)),
                ],
                txt: vec![("v".to_string(), "1".to_string())],
            }
        );
        let many: Vec<IpAddr> = (1..=20)
            .map(|host| IpAddr::V4(Ipv4Addr::new(10, 0, 0, host)))
            .collect();
        let capped = super::advertisement(&key, &fingerprint(), "h.local.", 1, &many, 0);
        assert_eq!(capped.addresses, many[..MAX_ADVERTISED_ADDRESSES].to_vec());
    }

    #[test]
    fn random_host_names_are_sixteen_hex_digits_under_local() {
        let first = random_host_name().expect("host name");
        let second = random_host_name().expect("host name");
        assert_ne!(first, second);
        let label = first.strip_suffix(".local.").expect("local domain");
        assert_eq!(label.len(), 16);
        assert!(
            label
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
    }

    #[test]
    fn instance_names_are_read_only_from_the_ganbaru_service_type() {
        assert_eq!(
            instance_name_of("1fbac9365d94a9541d55a6b31eb89632._ganbaru._tcp.local."),
            Some("1fbac9365d94a9541d55a6b31eb89632")
        );
        assert_eq!(instance_name_of("printer._ipp._tcp.local."), None);
        assert_eq!(instance_name_of("._ganbaru._tcp.local."), None);
        assert_eq!(instance_name_of("_ganbaru._tcp.local."), None);
    }

    #[test]
    fn candidates_come_only_from_expected_names_and_current_versions() {
        let expected = vec!["aa".to_string(), "bb".to_string()];
        let mut old_version = record("bb", 43_821, &["192.168.1.30"]);
        old_version.version = Some("2".to_string());
        let mut unversioned = record("aa", 43_821, &["192.168.1.31"]);
        unversioned.version = None;
        let records = vec![
            record("zz", 43_821, &["192.168.1.10"]),
            record("aa", 43_821, &["192.168.1.20"]),
            old_version,
            unversioned,
            record("bb", 43_822, &["10.0.0.5"]),
        ];
        assert_eq!(
            match_candidates(&expected, &records),
            endpoints(&["192.168.1.20:43821", "10.0.0.5:43822"])
        );
        assert!(match_candidates(&[], &records).is_empty());
    }

    #[test]
    fn candidates_pass_the_dial_policy_and_stop_at_the_cap() {
        let expected = vec!["aa".to_string()];
        let records = vec![record(
            "aa",
            43_821,
            &[
                "fe80::1",
                "127.0.0.1",
                "::1",
                "203.0.113.4",
                "8.8.8.8",
                "224.0.0.251",
                "fd00::20",
                "192.168.1.20",
                "::ffff:192.168.1.20",
            ],
        )];
        assert_eq!(
            match_candidates(&expected, &records),
            endpoints(&["192.168.1.20:43821", "[fd00::20]:43821"])
        );
        assert!(match_candidates(&expected, &[record("aa", 0, &["192.168.1.20"])]).is_empty());

        let many: Vec<String> = (1..=20).map(|host| format!("10.0.1.{host}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        let capped = match_candidates(&expected, &[record("aa", 43_821, &many)]);
        assert_eq!(capped.len(), MAX_DISCOVERED_ENDPOINTS);
        assert!(capped.iter().all(|endpoint| endpoint.port() == 43_821));
        assert!(!capped.contains(&SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 43_821)));
    }
}
