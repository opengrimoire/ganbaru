use super::*;

const IPV4_ROUTES: &str = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
wlp2s0\t00000000\tFE01A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
wlp2s0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
wlp2s0\tFE01A8C0\t00000000\t0005\t0\t0\t600\tFFFFFFFF\t0\t0\t0
eth0\t0000000A\t00000000\t0001\t0\t0\t0\t000000FE\t0\t0\t0
eth1\t0000000A\t00000000\t0001\t0\t0\t0\t00FF00FF\t0\t0\t0
eth2\t0002A8C0\t00000000\t0000\t0\t0\t0\t00FFFFFF\t0\t0\t0
bad/if\t0003A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0
";

const IPV6_ROUTES: &str = "\
2806101e000e89700000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000258 00000001 00000000 00400001   wlp6s0
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001   wlp6s0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 fe800000000000000000000000000001 00000258 00000009 00000000 00400003   wlp6s0
00000000000000000000000000000001 80 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 0000000b 00000000 80200001       lo
2806101e000e89704a89e7fffecf5d55 80 00000000000000000000000000000000 00 00000000000000000000000000000000 00000000 00000003 00000000 80200001   wlp6s0
ff000000000000000000000000000000 08 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000008 00000000 00000001   wlp6s0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200       lo
fd123456000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000001 00000000 00000001      wg0
";

const HOME: &str = "a4:2b:b0:11:22:33";
const CAFE: &str = "f0:9f:c2:44:55:66";
const GLOBAL: &str = "2806:101e:e:8970:4a89:e7ff:fecf:5d55";

fn prefix(value: &str) -> NetworkPrefix {
    value.parse().expect("network prefix")
}

fn host(value: &str) -> NetworkPrefix {
    NetworkPrefix::host(value.parse().expect("address"))
}

fn routes() -> Vec<Route> {
    let mut routes = parse_ipv4_routes(IPV4_ROUTES);
    routes.extend(parse_ipv6_routes(IPV6_ROUTES));
    routes
}

fn local(interface: &str, address: &str, prefix_len: u8) -> LocalInterfaceAddress {
    LocalInterfaceAddress {
        interface: interface.to_string(),
        address: address.parse().expect("address"),
        prefix_len,
    }
}

fn rules_for(interface: &str, address: &str, prefix_len: u8) -> Option<AddressRules> {
    address_rules(&local(interface, address, prefix_len), &routes())
}

fn tcp(interface: &str, destination: &str, source: &str) -> AuthorizedScope {
    AuthorizedScope {
        firewall: FirewallKind::Ufw,
        protocol: RuleProtocol::Tcp,
        interface: interface.to_string(),
        destination: NetworkPrefix::parse_rule_address(destination).expect("destination"),
        source: prefix(source),
        port: COORDINATOR_PORT,
        zone: None,
    }
}

fn udp(interface: &str, group: &str, source: &str) -> AuthorizedScope {
    AuthorizedScope {
        protocol: RuleProtocol::Udp,
        destination: host(group),
        port: MDNS_PORT,
        ..tcp(interface, group, source)
    }
}

fn numbered_scope(index: u8) -> AuthorizedScope {
    let network = format!("192.168.{index}.0/24");
    tcp("wlp2s0", &network, &network)
}

fn candidate(index: u8, network: Option<&str>) -> Candidate {
    Candidate {
        scope: numbered_scope(index),
        network: network.map(str::to_string),
    }
}

fn granted(index: u8, networks: &[&str]) -> GrantedScope {
    GrantedScope {
        rule: numbered_scope(index),
        networks: networks.iter().map(|network| network.to_string()).collect(),
    }
}

fn rules(stored: &StoredNetworkAccess) -> Vec<AuthorizedScope> {
    stored
        .scopes
        .iter()
        .map(|saved| saved.rule.clone())
        .collect()
}

fn helper_request(arguments: &[&str]) -> Result<HelperRequest, String> {
    parse_privileged_helper_request(&mut arguments.iter().map(|value| value.to_string()))
}

fn scratch_folder(name: &str) -> PathBuf {
    let folder = std::env::temp_dir().join(format!(
        "ganbaru-network-access-{name}-{}",
        std::process::id()
    ));
    fs::create_dir_all(&folder).unwrap();
    folder
}

#[test]
fn prefixes_are_aligned_and_hosts_print_bare() {
    assert_eq!(prefix("192.168.1.0/24").to_string(), "192.168.1.0/24");
    assert_eq!(prefix("fe80::/10").to_string(), "fe80::/10");
    for invalid in [
        "192.168.1.66/24",
        "10.0.0.0/33",
        "::/129",
        "fe80::1/10",
        "192.168.1.0",
        "192.168.1.0/",
        "example/24",
    ] {
        assert!(invalid.parse::<NetworkPrefix>().is_err(), "{invalid}");
    }
    assert_eq!(
        NetworkPrefix::containing("192.168.1.66".parse().unwrap(), 24),
        Some(prefix("192.168.1.0/24"))
    );
    assert_eq!(
        NetworkPrefix::containing("10.0.0.1".parse().unwrap(), 33),
        None
    );

    assert_eq!(host("192.168.1.66").rule_text(), "192.168.1.66");
    assert_eq!(host("ff02::fb").rule_text(), "ff02::fb");
    assert_eq!(prefix("192.168.1.66/32"), host("192.168.1.66"));
    assert_eq!(prefix("10.0.0.0/8").rule_text(), "10.0.0.0/8");
    assert_eq!(
        NetworkPrefix::parse_rule_address("192.168.1.66"),
        Some(host("192.168.1.66"))
    );
    assert_eq!(
        NetworkPrefix::parse_rule_address("2806:101e:e:8970::/64"),
        Some(prefix("2806:101e:e:8970::/64"))
    );
    assert_eq!(NetworkPrefix::parse_rule_address("192.168.1.66/24"), None);

    assert!(prefix("192.168.0.0/16").covers(&prefix("192.168.1.0/24")));
    assert!(!prefix("192.168.1.0/24").covers(&prefix("192.168.0.0/16")));
    assert!(!prefix("0.0.0.0/0").contains("::1".parse().unwrap()));

    let json = serde_json::to_string(&prefix("fc00::/7")).unwrap();
    assert_eq!(json, "\"fc00::/7\"");
    assert_eq!(
        serde_json::from_str::<NetworkPrefix>(&json).unwrap(),
        prefix("fc00::/7")
    );
    assert!(serde_json::from_str::<NetworkPrefix>("\"10.1.0.0/8\"").is_err());
}

#[test]
fn route_tables_keep_only_up_routes_that_reach_a_neighbor() {
    let route = |destination: &str, interface: &str, gateway: bool| Route {
        destination: prefix(destination),
        interface: interface.to_string(),
        gateway,
    };
    assert_eq!(
        parse_ipv4_routes(IPV4_ROUTES),
        vec![
            route("0.0.0.0/0", "wlp2s0", true),
            route("192.168.1.0/24", "wlp2s0", false),
            route("192.168.1.254/32", "wlp2s0", false),
            route("10.0.0.0/7", "eth0", false),
        ],
        "non-contiguous masks, down routes, and unsafe interface names are skipped"
    );
    assert_eq!(
        parse_ipv6_routes(IPV6_ROUTES),
        vec![
            route("2806:101e:e:8970::/64", "wlp6s0", false),
            route("fe80::/64", "wlp6s0", false),
            route("::/0", "wlp6s0", true),
            route("ff00::/8", "wlp6s0", false),
            route("fd12:3456::/64", "wg0", false),
        ],
        "local and reject routes are skipped"
    );
    assert!(parse_ipv6_routes("2806101e 40 x\n").is_empty());

    let routes = routes();
    assert!(has_default_route(&routes, "wlp6s0"));
    assert!(has_default_route(&routes, "wlp2s0"));
    assert!(!has_default_route(&routes, "wg0"));
    assert_eq!(
        connected_prefix(&routes, "wlp2s0", "192.168.1.66".parse().unwrap()),
        Some(24)
    );
    assert_eq!(
        connected_prefix(&routes, "wlp2s0", "192.168.1.254".parse().unwrap()),
        Some(32)
    );
    assert_eq!(
        connected_prefix(&routes, "wlp2s0", "10.1.2.3".parse().unwrap()),
        None,
        "a default route is never a connected network"
    );
    assert_eq!(
        connected_prefix(&routes, "wlp6s0", GLOBAL.parse().unwrap()),
        Some(64)
    );
}

#[test]
fn own_global_prefixes_are_the_blocks_of_global_interface_addresses() {
    let interfaces = "\
2806101e000e89704a89e7fffecf5d55 03 40 00 00   wlp6s0
fe80000000000000a2b3c4fffed5e6f7 03 40 20 80   wlp6s0
00000000000000000000000000000001 01 80 10 80       lo
fd123456000000000000000000000001 05 40 00 80      wg0
2a0012340000abcd0000000000000001 06 30 00 80     eth1
2a00123400000001 06 40 00 80     eth2
";
    assert_eq!(
        parse_own_global_ipv6_prefixes(interfaces),
        vec![
            prefix("2806:101e:e:8970::/64"),
            prefix("2a00:1234:0:abcd::/64")
        ],
        "link-local, loopback, unique local, and malformed rows add no prefix, and a wider \
             interface prefix still yields its /64"
    );
    assert!(parse_own_global_ipv6_prefixes("").is_empty());
}

#[test]
fn grants_open_a_global_ipv6_source_only_inside_an_own_prefix() {
    let own = [prefix("2806:101e:e:8970::/64")];
    let global = tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64");
    let foreign = tcp("wlp6s0", "2a00:1234:0:1::/64", "2a00:1234:0:1::/64");
    let narrower = tcp(
        "wlp6s0",
        "2806:101e:e:8970:4a89::/80",
        "2806:101e:e:8970:4a89::/80",
    );
    let private = [
        tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24"),
        tcp("wg0", "fd12:3456::/64", "fd00::/8"),
        udp("wlp6s0", "ff02::fb", "fe80::/10"),
    ];

    assert_eq!(
        validate_own_global_sources(std::slice::from_ref(&global), &own),
        Ok(())
    );
    assert_eq!(validate_own_global_sources(&[narrower], &own), Ok(()));
    assert!(
        validate_own_global_sources(&[global.clone(), foreign.clone()], &own).is_err(),
        "one foreign global source refuses the whole grant"
    );
    assert!(validate_own_global_sources(&[foreign], &own).is_err());
    assert!(
        validate_own_global_sources(&[global], &[]).is_err(),
        "a host without global addresses opens no global source"
    );
    assert_eq!(
        validate_own_global_sources(&private, &[]),
        Ok(()),
        "private, unique local, and link-local sources are not checked against own prefixes"
    );
    assert_eq!(
        validate_privileged_grant(&private),
        Ok(()),
        "a grant without global sources does not read the interfaces"
    );
}

#[test]
fn lan_addresses_open_their_connected_network_within_their_block() {
    let ipv4 = rules_for("wlp2s0", "192.168.1.66", 24).expect("IPv4 LAN rules");
    assert_eq!(ipv4.listener_destination, prefix("192.168.1.0/24"));
    assert_eq!(ipv4.listener_source, prefix("192.168.1.0/24"));
    assert_eq!(ipv4.discovery_source, Some(prefix("192.168.1.0/24")));

    let wide = rules_for("eth0", "10.1.2.3", 7).expect("wide connected route");
    assert_eq!(
        wide.listener_source,
        prefix("10.0.0.0/8"),
        "a connected route broader than its private block is narrowed to the block"
    );

    let global = rules_for("wlp6s0", GLOBAL, 64).expect("global IPv6 rules");
    assert_eq!(global.listener_destination, prefix("2806:101e:e:8970::/64"));
    assert_eq!(global.listener_source, prefix("2806:101e:e:8970::/64"));
    assert_eq!(global.discovery_source, Some(prefix("fe80::/10")));

    let dhcpv6 = rules_for("wlp6s0", "2806:101e:e:8970::1234", 128).expect("DHCPv6");
    assert_eq!(
        dhcpv6.listener_source,
        prefix("2806:101e:e:8970::/64"),
        "a /128 lease opens the connected route that holds it"
    );

    let unrouted = rules_for("eth9", "2806:101e:e:8970::1", 48).expect("unrouted");
    assert_eq!(
        unrouted.listener_source,
        prefix("2806:101e:e:8970::/64"),
        "a global prefix never extends past its on-link /64"
    );

    let routed_ula = rules_for("wlp6s0", "fd12:3456::1", 64).expect("routed ULA");
    assert_eq!(routed_ula.listener_source, prefix("fd12:3456::/64"));
    assert_eq!(routed_ula.discovery_source, Some(prefix("fe80::/10")));

    for (interface, address, prefix_len) in [
        ("wlp6s0", "fe80::4a89:e7ff:fecf:5d55", 64),
        ("lo", "127.0.0.1", 8),
        ("lo", "::1", 128),
        ("eth0", "8.8.8.8", 24),
        ("eth0", "2001:db8::1", 64),
        ("bad/if", "192.168.1.66", 24),
        ("wlp2s0", "0.0.0.0", 0),
    ] {
        assert_eq!(
            rules_for(interface, address, prefix_len),
            None,
            "{address} on {interface}"
        );
    }
}

#[test]
fn overlay_addresses_admit_their_block_on_their_interface_without_discovery() {
    let tailscale = rules_for("tailscale0", "100.101.2.3", 32).expect("Tailscale");
    assert_eq!(tailscale.interface, "tailscale0");
    assert_eq!(tailscale.listener_destination, host("100.101.2.3"));
    assert_eq!(tailscale.listener_source, prefix("100.64.0.0/10"));
    assert_eq!(tailscale.discovery_source, None);

    let tailscale_ipv6 =
        rules_for("tailscale0", "fd7a:115c:a1e0::4a01:c903", 128).expect("Tailscale IPv6");
    assert_eq!(
        tailscale_ipv6.listener_destination,
        host("fd7a:115c:a1e0::4a01:c903")
    );
    assert_eq!(tailscale_ipv6.listener_source, prefix("fc00::/7"));
    assert_eq!(tailscale_ipv6.discovery_source, None);

    let wireguard = rules_for("wg0", "fd12:3456::1", 64).expect("unique local overlay");
    assert_eq!(
        wireguard.listener_destination,
        prefix("fd12:3456::/64"),
        "unique local without a default route is an overlay"
    );
    assert_eq!(wireguard.listener_source, prefix("fc00::/7"));
    assert_eq!(wireguard.discovery_source, None);
}

#[test]
fn scopes_list_listeners_first_once_each_for_every_active_firewall() {
    let addresses = [
        local("wlp2s0", "192.168.1.66", 24),
        local("wlp6s0", GLOBAL, 64),
        local("wlp6s0", "2806:101e:e:8970:1c2d:3e4f:5a6b:7c8d", 64),
        local("tailscale0", "100.101.2.3", 32),
    ];
    let routes = routes();
    let rules: Vec<AddressRules> = addresses
        .iter()
        .filter_map(|address| address_rules(address, &routes))
        .collect();
    assert_eq!(rules.len(), 4);

    let ipv4_only = rule_scopes(
        &rules,
        COORDINATOR_PORT,
        &[ActiveFirewall {
            kind: FirewallKind::Ufw,
            ipv6: false,
        }],
        |_| panic!("UFW has no zones"),
    );
    assert_eq!(
        ipv4_only,
        vec![
            tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24"),
            tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
            udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
        ],
        "a UFW without IPv6 filtering gets no IPv6 rule"
    );

    let mut zone_lookups = Vec::new();
    let both = rule_scopes(
        &rules,
        DEVELOPMENT_COORDINATOR_PORT,
        &[
            ActiveFirewall {
                kind: FirewallKind::Ufw,
                ipv6: true,
            },
            ActiveFirewall {
                kind: FirewallKind::Firewalld,
                ipv6: true,
            },
        ],
        |interface| {
            zone_lookups.push(interface.to_string());
            Some("home".to_string())
        },
    );
    assert_eq!(zone_lookups, ["wlp2s0", "wlp6s0", "tailscale0"]);
    assert_eq!(
        both.len(),
        10,
        "two addresses in one /64 share their scopes"
    );
    let tcp_count = both
        .iter()
        .take_while(|scope| scope.protocol == RuleProtocol::Tcp)
        .count();
    assert_eq!(tcp_count, 6);
    assert!(
        both[tcp_count..]
            .iter()
            .all(|scope| scope.protocol == RuleProtocol::Udp && scope.port == MDNS_PORT)
    );
    assert!(both.contains(&AuthorizedScope {
        firewall: FirewallKind::Firewalld,
        zone: Some("home".to_string()),
        ..udp("wlp6s0", "ff02::fb", "fe80::/10")
    }));
    assert!(both.contains(&AuthorizedScope {
        port: DEVELOPMENT_COORDINATOR_PORT,
        ..tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64")
    }));
    for scope in &both {
        assert!(validate_authorized_scope(scope).is_ok(), "{scope:?}");
    }
}

#[test]
fn the_validator_accepts_only_listener_and_discovery_rules_from_local_sources() {
    for valid in [
        tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
        tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24"),
        tcp("eth0", "10.0.0.0/8", "10.0.0.0/8"),
        tcp("eth0", "172.20.1.0/24", "172.16.0.0/12"),
        tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
        tcp("tailscale0", "fd7a:115c:a1e0::1", "fc00::/7"),
        tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64"),
        AuthorizedScope {
            port: DEVELOPMENT_COORDINATOR_PORT,
            ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
        },
        udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
        udp("wlp2s0", "224.0.0.251", "169.254.0.0/16"),
        udp("wlp6s0", "ff02::fb", "fe80::/10"),
        udp("wlp6s0", "ff02::fb", "2806:101e:e:8970::/64"),
    ] {
        assert!(validate_authorized_scope(&valid).is_ok(), "{valid:?}");
    }

    let base = tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24");
    let discovery = udp("wlp2s0", "224.0.0.251", "192.168.1.0/24");
    let invalid = [
        (
            "public IPv4 source",
            tcp("eth0", "8.8.8.0/24", "8.8.8.0/24"),
        ),
        ("any source", tcp("eth0", "192.168.1.66", "0.0.0.0/0")),
        ("any IPv6 source", tcp("eth0", "fd00::1", "::/0")),
        (
            "source wider than its block",
            tcp("eth0", "10.1.2.3", "10.0.0.0/7"),
        ),
        (
            "overlay source wider than its block",
            tcp("eth0", "100.101.2.3", "100.0.0.0/8"),
        ),
        (
            "global source wider than /64",
            tcp("eth0", "2806:101e:e::/64", "2806:101e:e::/48"),
        ),
        (
            "documentation source",
            tcp("eth0", "2001:db8::/64", "2001:db8::/64"),
        ),
        ("multicast source", tcp("eth0", "ff02::fb", "ff02::/16")),
        ("loopback source", tcp("lo", "127.0.0.1", "127.0.0.0/8")),
        (
            "destination outside its source",
            tcp("eth0", "192.168.2.0/24", "192.168.1.0/24"),
        ),
        (
            "destination of another family",
            tcp("eth0", "fd00::1", "192.168.1.0/24"),
        ),
        (
            "other TCP port",
            AuthorizedScope {
                port: 44_000,
                ..base.clone()
            },
        ),
        (
            "discovery port over TCP",
            AuthorizedScope {
                port: MDNS_PORT,
                ..base.clone()
            },
        ),
        (
            "coordinator port over UDP",
            AuthorizedScope {
                port: COORDINATOR_PORT,
                ..discovery.clone()
            },
        ),
        (
            "UDP to a unicast destination",
            udp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
        ),
        (
            "UDP to the other family's group",
            udp("wlp2s0", "ff02::fb", "192.168.1.0/24"),
        ),
        (
            "UDP from a public source",
            udp("eth0", "224.0.0.251", "8.8.8.0/24"),
        ),
        (
            "empty interface",
            AuthorizedScope {
                interface: String::new(),
                ..base.clone()
            },
        ),
        (
            "unsafe interface",
            AuthorizedScope {
                interface: "eth0;reboot".to_string(),
                ..discovery
            },
        ),
        (
            "long interface",
            AuthorizedScope {
                interface: "a".repeat(16),
                ..base.clone()
            },
        ),
        (
            "UFW with a zone",
            AuthorizedScope {
                zone: Some("home".to_string()),
                ..base.clone()
            },
        ),
        (
            "firewalld without a zone",
            AuthorizedScope {
                firewall: FirewallKind::Firewalld,
                ..base.clone()
            },
        ),
        (
            "firewalld with an unsafe zone",
            AuthorizedScope {
                firewall: FirewallKind::Firewalld,
                zone: Some("home zone".to_string()),
                ..base
            },
        ),
    ];
    for (reason, scope) in invalid {
        assert!(validate_authorized_scope(&scope).is_err(), "{reason}");
    }
}

#[test]
fn the_helper_parses_only_complete_valid_scopes() {
    let valid = [
        "grant",
        "ufw",
        "tcp",
        "wlp2s0",
        "192.168.1.0/24",
        "192.168.1.0/24",
        "43822",
        "-",
        "firewalld",
        "udp",
        "wlp6s0",
        "ff02::fb/128",
        "fe80::/10",
        "5353",
        "home",
    ];
    let Ok(HelperRequest::Apply { scopes, grant }) = helper_request(&valid) else {
        panic!("valid helper scopes");
    };
    assert!(grant);
    assert_eq!(
        scopes,
        vec![
            AuthorizedScope {
                port: DEVELOPMENT_COORDINATOR_PORT,
                ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
            },
            AuthorizedScope {
                firewall: FirewallKind::Firewalld,
                zone: Some("home".to_string()),
                ..udp("wlp6s0", "ff02::fb", "fe80::/10")
            },
        ]
    );

    let replace = |index: usize, value: &'static str| {
        let mut arguments = valid;
        arguments[index] = value;
        helper_request(&arguments)
    };
    for (index, value) in [
        (0, "allow"),
        (1, "iptables"),
        (2, "icmp"),
        (2, "TCP"),
        (3, "wlp2s0;id"),
        (4, "192.168.1.66/24"),
        (4, "192.168.1.66"),
        (5, "0.0.0.0/0"),
        (5, "8.8.8.0/24"),
        (6, "22"),
        (6, "43823"),
        (6, "5353"),
        (6, "port"),
        (7, "home"),
        (9, "tcp"),
        (12, "fe80::/9"),
        (13, "53"),
        (14, "-"),
    ] {
        assert!(
            replace(index, value).is_err(),
            "argument {index} as {value}"
        );
    }
    assert!(
        helper_request(&valid[..valid.len() - 1]).is_err(),
        "truncated"
    );
    let mut extra = valid.to_vec();
    extra.push("unexpected");
    assert!(helper_request(&extra).is_err(), "trailing argument");
    assert!(helper_request(&["grant"]).is_err());

    assert_eq!(
        helper_request(&["revoke-all"]),
        Ok(HelperRequest::RevokeAll)
    );
    assert!(helper_request(&["revoke-all", "ufw"]).is_err());
}

#[test]
fn one_helper_invocation_carries_every_scope_of_a_flow() {
    let firewalld = AuthorizedScope {
        firewall: FirewallKind::Firewalld,
        zone: Some("home".to_string()),
        ..numbered_scope(1)
    };
    let scopes = vec![
        numbered_scope(1),
        firewalld,
        AuthorizedScope {
            port: DEVELOPMENT_COORDINATOR_PORT,
            ..tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64")
        },
        udp("wlp6s0", "ff02::fb", "fe80::/10"),
        tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
    ];
    for grant in [true, false] {
        let arguments = privileged_helper_arguments(&scopes, grant).expect("arguments");
        assert_eq!(arguments[0], PRIVILEGED_HELPER_ARGUMENT);
        assert_eq!(arguments.len(), 2 + 7 * scopes.len());
        let request = parse_privileged_helper_request(&mut arguments.into_iter().skip(1));
        assert_eq!(
            request,
            Ok(HelperRequest::Apply {
                scopes: scopes.clone(),
                grant
            })
        );
    }

    let most: Vec<AuthorizedScope> = (0..MAX_AUTHORIZED_SCOPES)
        .map(|index| numbered_scope(u8::try_from(index).unwrap()))
        .collect();
    let arguments = privileged_helper_arguments(&most, false).expect("bounded arguments");
    let mut too_many: Vec<String> = arguments.into_iter().skip(1).collect();
    too_many.extend(too_many[1..8].to_vec());
    assert!(parse_privileged_helper_request(&mut too_many.into_iter()).is_err());
    assert!(privileged_helper_arguments(&[], true).is_err());
    assert!(privileged_helper_arguments(&[most.clone(), most].concat(), true).is_err());
    let other_port = AuthorizedScope {
        port: 8080,
        ..numbered_scope(1)
    };
    assert!(privileged_helper_arguments(&[other_port], true).is_err());
}

#[test]
fn a_grant_stops_at_a_failed_listener_rule_but_not_at_a_failed_discovery_rule() {
    let discovery = udp("wlp2s0", "224.0.0.251", "192.168.1.0/24");
    let scopes = [
        numbered_scope(1),
        numbered_scope(2),
        discovery.clone(),
        numbered_scope(3),
    ];
    let run = |grant: bool, failures: &[AuthorizedScope]| {
        let mut attempted = Vec::new();
        let result = apply_each(&scopes, grant, |scope| {
            attempted.push(scope.clone());
            if failures.contains(scope) {
                Err(format!("failed {}", scope.source))
            } else {
                Ok(())
            }
        });
        (result, attempted)
    };

    let (result, attempted) = run(true, std::slice::from_ref(&discovery));
    assert_eq!(result, Ok(()));
    assert_eq!(attempted, scopes);

    let failures = [numbered_scope(2), discovery];
    let (result, attempted) = run(true, &failures);
    assert_eq!(result, Err("failed 192.168.2.0/24".to_string()));
    assert_eq!(attempted, scopes[..2]);

    let (result, attempted) = run(false, &failures);
    assert_eq!(result, Err("failed 192.168.2.0/24".to_string()));
    assert_eq!(attempted, scopes, "a revoke attempts every scope");
}

#[test]
fn a_full_record_refuses_a_grant_before_any_rule_changes() {
    let last = u8::try_from(MAX_AUTHORIZED_SCOPES).unwrap() - 2;
    let mut stored = empty_state();
    stored.scopes = (0..last).map(|index| granted(index, &[])).collect();
    stored.pending = vec![numbered_scope(last)];
    let mut applied = Vec::new();
    let result = grant_scopes(
        &mut stored,
        vec![candidate(last + 1, None), candidate(last + 2, None)],
        |_| Ok(()),
        |scopes| {
            applied.extend_from_slice(scopes);
            Ok(())
        },
    );
    assert!(result.is_err());
    assert!(applied.is_empty());
    assert_eq!(
        stored.scopes.len() + stored.pending.len(),
        MAX_AUTHORIZED_SCOPES - 1
    );

    let granted = grant_scopes(
        &mut stored,
        vec![candidate(last, None), candidate(last + 1, None)],
        |_| Ok(()),
        |scopes| {
            applied.extend_from_slice(scopes);
            Ok(())
        },
    );
    assert!(granted.is_ok());
    assert_eq!(
        applied,
        vec![numbered_scope(last), numbered_scope(last + 1)]
    );
    assert_eq!(stored.scopes.len(), MAX_AUTHORIZED_SCOPES);
    assert!(stored.pending.is_empty());
}

#[test]
fn one_flow_applies_its_scopes_in_one_call_and_a_failed_call_stays_revocable() {
    let mut stored = empty_state();
    let mut persisted: Vec<(usize, usize)> = Vec::new();
    let mut calls = Vec::new();
    let result = grant_scopes(
        &mut stored,
        vec![candidate(1, Some(HOME)), candidate(2, Some(HOME))],
        |state| {
            persisted.push((state.scopes.len(), state.pending.len()));
            Ok(())
        },
        |scopes| {
            calls.push(scopes.to_vec());
            Err("firewalld failed after the runtime rule".to_string())
        },
    );
    assert!(result.is_err());
    assert_eq!(calls, vec![vec![numbered_scope(1), numbered_scope(2)]]);
    assert_eq!(persisted, vec![(0, 2)]);
    assert!(stored.scopes.is_empty());
    assert_eq!(stored.pending, vec![numbered_scope(1), numbered_scope(2)]);

    let granted_again = grant_scopes(
        &mut stored,
        vec![
            candidate(1, Some(HOME)),
            candidate(2, Some(HOME)),
            candidate(3, Some(CAFE)),
        ],
        |state| {
            persisted.push((state.scopes.len(), state.pending.len()));
            Ok(())
        },
        |scopes| {
            calls.push(scopes.to_vec());
            Ok(())
        },
    );
    assert!(granted_again.is_ok());
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[1].len(), 3);
    assert_eq!(persisted[1..], [(0, 3), (3, 0)]);
    assert_eq!(
        stored.scopes,
        vec![
            granted(1, &[HOME]),
            granted(2, &[HOME]),
            granted(3, &[CAFE])
        ],
        "each scope records the network of its own interface"
    );

    stored.pending = vec![numbered_scope(4)];
    let mut removals = Vec::new();
    let revoked = revoke_scopes(&mut stored, |scopes| {
        removals.push(scopes.to_vec());
        Err("authorization was cancelled".to_string())
    });
    assert!(revoked.is_err());
    assert_eq!(rules(&stored).len(), 3);
    assert_eq!(stored.pending, vec![numbered_scope(4)]);

    let revoked = revoke_scopes(&mut stored, |scopes| {
        removals.push(scopes.to_vec());
        Ok(())
    });
    assert!(revoked.is_ok());
    assert_eq!(removals.len(), 2);
    assert_eq!(
        removals[1],
        [1, 2, 3, 4].map(numbered_scope).to_vec(),
        "one call removes every granted and pending rule"
    );
    assert!(stored.scopes.is_empty() && stored.pending.is_empty());
    assert!(revoke_scopes(&mut stored, |_| panic!("nothing to remove")).is_ok());
}

#[test]
fn rules_admit_peers_only_on_networks_the_user_confirmed() {
    let mut stored = empty_state();
    stored.scopes = vec![granted(1, &[HOME])];
    let state = |stored: &StoredNetworkAccess, candidates: &[Candidate], can: bool| {
        access_state(stored, candidates, || can)
    };
    assert_eq!(
        state(&stored, &[candidate(1, Some(HOME))], false),
        NetworkAccessState::Granted
    );
    assert_eq!(
        state(&stored, &[candidate(1, Some(CAFE))], false),
        NetworkAccessState::NetworkConfirmationRequired
    );
    assert_eq!(
        state(&stored, &[candidate(1, None)], false),
        NetworkAccessState::Granted
    );
    let missing = [candidate(1, Some(HOME)), candidate(2, Some(HOME))];
    assert_eq!(
        state(&stored, &missing, true),
        NetworkAccessState::AuthorizationRequired
    );
    assert_eq!(
        state(&stored, &missing, false),
        NetworkAccessState::ManualActionRequired
    );
    let discovery = Candidate {
        scope: udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
        network: Some(HOME.to_string()),
    };
    assert_eq!(
        state(&stored, &[candidate(1, Some(HOME)), discovery], true),
        NetworkAccessState::Granted,
        "a missing discovery rule never blocks linking"
    );

    stored.scopes = vec![granted(1, &[])];
    assert_eq!(
        state(&stored, &[candidate(1, Some(HOME))], false),
        NetworkAccessState::NetworkConfirmationRequired
    );
    assert!(
        admits_lan_peers(
            Path::new("/nonexistent"),
            &[local("wlp2s0", "192.168.1.66", 24)],
            "10.0.0.2".parse().unwrap(),
            COORDINATOR_PORT
        ),
        "a local address without an interface entry has no rule to confirm"
    );
}

#[test]
fn confirming_a_network_records_it_without_changing_the_firewall() {
    let mut stored = empty_state();
    stored.scopes = vec![granted(1, &[HOME])];
    let mut persisted = 0;
    let confirmed = grant_scopes(
        &mut stored,
        vec![candidate(1, Some(CAFE))],
        |_| {
            persisted += 1;
            Ok(())
        },
        |_| panic!("an existing rule is not applied again"),
    );
    assert!(confirmed.is_ok());
    assert_eq!(persisted, 1);
    assert_eq!(stored.scopes, vec![granted(1, &[HOME, CAFE])]);
    let json = serde_json::to_vec(&stored).expect("serialize");
    let restored: StoredNetworkAccess = serde_json::from_slice(&json).expect("parse");
    assert_eq!(restored.scopes, stored.scopes);

    let mut saved = granted(1, &[]);
    let identities: Vec<String> = (0..=MAX_ALLOWED_NETWORKS)
        .map(|index| format!("02:00:00:00:00:{index:02x}"))
        .collect();
    for identity in &identities {
        assert!(allow_network(&mut saved, Some(identity)));
    }
    assert!(!allow_network(&mut saved, Some(&identities[1])));
    assert!(!allow_network(&mut saved, None));
    assert_eq!(saved.networks, identities[1..]);
}

#[test]
fn saved_state_of_another_schema_version_is_refused() {
    let folder = scratch_folder("state");
    let mut stored = empty_state();
    stored.scopes = vec![granted(1, &[HOME])];
    stored.pending = vec![udp("wlp6s0", "ff02::fb", "fe80::/10")];
    persist_state(&folder, &stored).expect("persist");
    let restored = read_state(&folder).expect("read");
    assert_eq!(restored.scopes, stored.scopes);
    assert_eq!(restored.pending, stored.pending);

    let earlier = r#"{"schemaVersion":2,"scopes":[{"firewall":"ufw","interface":"wlp2s0","localAddress":"192.168.1.66","sourceNetwork":"192.168.1.0/24","port":43821,"networks":[]}],"pending":[]}"#;
    fs::write(state_path(&folder), earlier).unwrap();
    let error = read_state(&folder).expect_err("schema version 2");
    assert!(error.contains("unsupported"), "{error}");

    let broad = serde_json::to_string(&StoredNetworkAccess {
        schema_version: STATE_SCHEMA_VERSION,
        scopes: Vec::new(),
        pending: vec![tcp("eth0", "192.168.1.66", "0.0.0.0/0")],
    })
    .unwrap();
    fs::write(state_path(&folder), broad).unwrap();
    assert!(read_state(&folder).is_err(), "saved scopes are validated");
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn the_network_identity_is_the_resolved_default_gateway_of_the_interface() {
    let routes = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
docker0\t00000000\t010011AC\t0003\t0\t0\t50\t00000000\t0\t0\t0
wlp2s0\t0001A8C0\t00000000\t0001\t0\t0\t600\t00FFFFFF\t0\t0\t0
wlp2s0\t00000000\t0101A8C0\t0003\t0\t0\t600\t00000000\t0\t0\t0
";
    assert_eq!(
        parse_default_gateway(routes, "wlp2s0"),
        Some(Ipv4Addr::new(192, 168, 1, 1))
    );
    assert_eq!(parse_default_gateway(routes, "eth0"), None);
    let down = routes.replace("0101A8C0\t0003", "0101A8C0\t0002");
    assert_eq!(parse_default_gateway(&down, "wlp2s0"), None);

    let neighbors = "\
IP address       HW type     Flags       HW address            Mask     Device
192.168.1.1      0x1         0x2         A4:2B:B0:11:22:33     *        wlp2s0
192.168.1.7      0x1         0x0         00:00:00:00:00:00     *        wlp2s0
172.17.0.1       0x1         0x2         f0:9f:c2:44:55:66     *        docker0
";
    let gateway = Ipv4Addr::new(192, 168, 1, 1);
    assert_eq!(
        parse_neighbor_hardware_address(neighbors, gateway, "wlp2s0").as_deref(),
        Some(HOME)
    );
    assert_eq!(
        parse_neighbor_hardware_address(neighbors, gateway, "eth0"),
        None
    );
    assert_eq!(
        parse_neighbor_hardware_address(neighbors, Ipv4Addr::new(192, 168, 1, 7), "wlp2s0"),
        None
    );
    let unresolved = neighbors.replace("0x2         A4", "0x0         A4");
    assert_eq!(
        parse_neighbor_hardware_address(&unresolved, gateway, "wlp2s0"),
        None
    );
    assert!(!is_network_identity("00:00:00:00:00:00"));
    assert!(!is_network_identity("A4:2B:B0:11:22:33"));
    assert!(!is_network_identity("a4:2b:b0:11:22"));
}

#[test]
fn firewall_commands_name_the_protocol_interface_and_both_prefixes() {
    assert_eq!(
        ufw_arguments(
            &AuthorizedScope {
                port: DEVELOPMENT_COORDINATOR_PORT,
                ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
            },
            true
        ),
        [
            "allow",
            "in",
            "on",
            "wlp2s0",
            "proto",
            "tcp",
            "from",
            "192.168.1.0/24",
            "to",
            "192.168.1.0/24",
            "port",
            "43822",
            "comment",
            "Ganbaru device linking",
        ]
    );
    let discovery = udp("wlp6s0", "ff02::fb", "fe80::/10");
    assert_eq!(
        ufw_arguments(&discovery, false)[..12],
        [
            "delete",
            "allow",
            "in",
            "on",
            "wlp6s0",
            "proto",
            "udp",
            "from",
            "fe80::/10",
            "to",
            "ff02::fb",
            "port",
        ]
        .map(str::to_string)
    );
    assert_eq!(
        firewalld_rich_rule(&discovery),
        "rule family=\"ipv6\" source address=\"fe80::/10\" destination address=\"ff02::fb/128\" port port=\"5353\" protocol=\"udp\" accept"
    );
}

#[test]
fn configuration_flags_ignore_comments_and_other_values() {
    assert!(!config_value_is_yes(
        "# ENABLED=yes\nENABLED=no\n",
        "ENABLED"
    ));
    assert!(config_value_is_yes("ENABLED=yes\n", "ENABLED"));
    assert!(config_value_is_yes("IPV6=\"yes\"\n", "IPV6"));
    assert!(!config_value_is_yes("IPV6=yes\n", "ENABLED"));
    assert!(!config_value_is_yes("NOIPV6=yes\n", "IPV6"));
}

#[test]
fn only_root_owned_entries_that_others_cannot_write_are_trusted() {
    use std::os::unix::fs::PermissionsExt;

    assert!(root_controlled(&Path::new("/").symlink_metadata().unwrap()));
    assert_ne!(PACKAGED_HELPER_PATH, PACKAGED_APP_PATH);
    assert!(
        PKEXEC_PATHS
            .iter()
            .all(|path| !path.starts_with("/usr/local/"))
    );
    assert_eq!(
        command_path(&["/nonexistent/pkexec", "/usr/bin/env"]),
        Some(PathBuf::from("/usr/bin/env"))
    );

    let folder = scratch_folder("command");
    let planted = folder.join("pkexec");
    fs::write(&planted, b"#!/bin/sh\n").unwrap();
    fs::set_permissions(&planted, fs::Permissions::from_mode(0o755)).unwrap();
    if planted.symlink_metadata().unwrap().uid() != 0 {
        assert!(!root_controlled(&folder.symlink_metadata().unwrap()));
        assert_eq!(command_path(&[planted.to_str().unwrap()]), None);
    }
    fs::remove_dir_all(&folder).unwrap();
}

#[test]
fn package_removal_finds_every_app_ufw_rule_shape() {
    let output = "\
Added user rules (see 'ufw status' for running firewall):
ufw allow 22/tcp
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.0/24 port 43822 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 224.0.0.251 port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from 2806:101e:e:8970::/64 to 2806:101e:e:8970::/64 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from fe80::/10 to ff02::fb port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on tailscale0 from 100.64.0.0/10 to 100.101.2.3 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'My own rule'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 8080 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 0.0.0.0/0 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp6s0 from ::/0 to ff02::fb port 5353 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 port 43821 to 192.168.1.66 proto tcp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto udp comment 'Ganbaru device linking'
ufw allow in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 5353 proto udp comment 'Ganbaru device linking'
ufw deny in on wlp2s0 from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
ufw allow from 192.168.1.0/24 to 192.168.1.66 port 43821 proto tcp comment 'Ganbaru device linking'
";
    assert_eq!(
        parse_ufw_added_rules(output),
        vec![
            tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
            AuthorizedScope {
                port: DEVELOPMENT_COORDINATOR_PORT,
                ..tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")
            },
            udp("wlp2s0", "224.0.0.251", "192.168.1.0/24"),
            tcp("wlp6s0", "2806:101e:e:8970::/64", "2806:101e:e:8970::/64"),
            udp("wlp6s0", "ff02::fb", "fe80::/10"),
            tcp("tailscale0", "100.101.2.3", "100.64.0.0/10"),
        ]
    );

    for scope in [
        tcp("wlp2s0", "192.168.1.66", "192.168.1.0/24"),
        udp("wlp6s0", "ff02::fb", "fe80::/10"),
    ] {
        let line = format!(
            "ufw {} '{RULE_COMMENT}'",
            ufw_arguments(&scope, true)[..13].join(" ")
        );
        assert_eq!(parse_ufw_added_rules(&line), vec![scope], "{line}");
    }
}

#[test]
fn package_removal_finds_every_app_firewalld_rule_shape() {
    let earlier = "rule family=\"ipv4\" source address=\"192.168.1.0/24\" destination address=\"192.168.1.66/32\" port port=\"43821\" protocol=\"tcp\" accept";
    let scope = parse_firewalld_rich_rule("home", earlier).expect("earlier host rule");
    assert_eq!(scope.zone.as_deref(), Some("home"));
    assert_eq!(scope.destination, host("192.168.1.66"));
    assert_eq!(scope.source, prefix("192.168.1.0/24"));
    let bare = earlier.replace("192.168.1.66/32", "192.168.1.66");
    assert_eq!(parse_firewalld_rich_rule("home", &bare), Some(scope));

    let zoned = |scope: AuthorizedScope| AuthorizedScope {
        firewall: FirewallKind::Firewalld,
        interface: String::new(),
        zone: Some("home".to_string()),
        ..scope
    };
    for scope in [
        zoned(tcp("wlp2s0", "192.168.1.0/24", "192.168.1.0/24")),
        zoned(udp("wlp2s0", "224.0.0.251", "192.168.1.0/24")),
        zoned(tcp(
            "wlp6s0",
            "2806:101e:e:8970::/64",
            "2806:101e:e:8970::/64",
        )),
        zoned(udp("wlp6s0", "ff02::fb", "fe80::/10")),
        zoned(tcp("tailscale0", "fd7a:115c:a1e0::1", "fc00::/7")),
    ] {
        let rule = firewalld_rich_rule(&scope);
        assert_eq!(
            parse_firewalld_rich_rule("home", &rule),
            Some(scope),
            "{rule}"
        );
    }

    let ipv6 = "rule family=\"ipv6\" source address=\"fe80::/10\" destination address=\"ff02::fb\" port port=\"5353\" protocol=\"udp\" accept";
    assert!(parse_firewalld_rich_rule("public", ipv6).is_some());
    for foreign in [
        earlier.replace("43821", "8080"),
        earlier.replace("accept", "reject"),
        earlier.replace("\"tcp\"", "\"udp\""),
        earlier.replace("192.168.1.0/24", "0.0.0.0/0"),
        earlier.replace("ipv4", "ipv6"),
        ipv6.replace("ipv6", "ipv4"),
        ipv6.replace("ff02::fb", "ff02::1"),
        ipv6.replace("5353", "43821"),
        ipv6.replace("fe80::/10", "::/0"),
        format!("{earlier} log"),
        "rule service name=\"ssh\" accept".to_string(),
    ] {
        assert_eq!(
            parse_firewalld_rich_rule("home", &foreign),
            None,
            "{foreign}"
        );
    }
    assert_eq!(parse_firewalld_rich_rule("bad zone", earlier), None);
}
